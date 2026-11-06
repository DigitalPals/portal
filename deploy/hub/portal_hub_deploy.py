#!/usr/bin/env python3
"""Portal managed Hub deployment protocol v1. Python 3 standard library only.

Stream authenticated source to isolated Python under root/sudo, with --request-stdin. It never
uses a shell, installs itself as a service, or accepts Tailscale account secrets.
"""

import argparse
import contextlib
import copy
import fcntl
import hashlib
import ipaddress
import io
import json
import os
from pathlib import Path
import platform
import re
import secrets
import selectors
import shutil
import signal
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
from urllib.parse import urlsplit
import uuid


PROTOCOL = 1
ROOT = Path("/opt/portal-hub")
DEFAULT_IMAGE = "ghcr.io/digitalpals/portal-hub:0.5.0-beta.17"
# Verified release AND Docker Hub multiarch manifest, 2026-11-06. See README.
TAILSCALE_IMAGE = "tailscale/tailscale:v1.104.4@sha256:ed6480b33c6eb67c9cbd203ed42b9fb57e361986725b6856def71b7a3d9e276c"
HUB_USER = "10001:10001"
MANAGER = "portal-hub-deploy-v1"
LABEL = "io.portal.hub."
OUTPUT_LIMIT = 65536
REQUEST_LIMIT = 16384
SAFE_ENV = {"PATH": "/usr/sbin:/usr/bin:/sbin:/bin", "HOME": "/root",
            "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "DEBIAN_FRONTEND": "noninteractive",
            "DOCKER_HOST": "unix:///var/run/docker.sock"}
ACTIONS = {"inspect", "install", "status", "configure_https", "claim", "doctor",
           "logs", "update", "backup", "uninstall"}
READ_ONLY = {"inspect", "status", "doctor", "logs"}
SEMVER = re.compile(r"v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*))?(?:\+[0-9A-Za-z.-]+)?\Z")
DIGEST = re.compile(r"sha256:[0-9a-f]{64}\Z")


class DeployError(Exception):
    def __init__(self, code, message, data=None):
        super().__init__(message)
        self.code, self.data = code, data or {}


def fail(code, message, data=None):
    raise DeployError(code, message, data)


def redact(text, secret_values=()):
    text = str(text)
    for value in secret_values:
        if value:
            text = text.replace(value, "[redacted]")
    text = re.sub(r"-----BEGIN [^-]*PRIVATE KEY-----.*?(?:-----END [^-]*PRIVATE KEY-----|\Z)",
                  "[redacted private key]", text, flags=re.S)
    text = re.sub(r"(?i)(bearer\s+)[^\s\"',]+", r"\1[redacted]", text)
    text = re.sub(r"(?i)((?:password|passwd|token|secret|authkey|auth_key|authorization|cookie)[\"']?\s*[:=]\s*)(?:\"[^\"]*\"|'[^']*'|[^\s,}]+)",
                  r"\1[redacted]", text)
    text = re.sub(r"tskey-[A-Za-z0-9_-]+", "[redacted]", text)
    # Enrollment/consent URLs are intentionally returned only in validated fields.
    text = re.sub(r"https?://[^\s\"'<>]+", "[redacted URL]", text)
    # Remove terminal controls as these diagnostics are displayed in the desktop.
    text = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]|[\x00-\x08\x0b-\x1f\x7f]", "", text)
    # Leave room for JSON escaping and the response envelope in the desktop's
    # 128 KiB transport limit. Limit bytes as well as Unicode characters.
    return text.encode("utf-8", "replace")[-OUTPUT_LIMIT // 2:].decode("utf-8", "ignore")


class Result:
    def __init__(self, returncode=0, stdout="", stderr="", truncated=False):
        self.returncode, self.stdout, self.stderr = returncode, stdout, stderr
        self.truncated = truncated


class Runner:
    """Drain output continuously into bounded tails, with deadline and cancellation."""
    def __init__(self, secret_values=()):
        self.secret_values = secret_values
        self.diagnostics = ""

    def run(self, argv, *, stdin=None, timeout=60, check=True):
        if not isinstance(argv, list) or not all(isinstance(x, str) for x in argv):
            raise TypeError("argv must be a string list")
        command = Path(argv[0]).name
        try:
            proc = subprocess.Popen(argv, stdin=(subprocess.PIPE if isinstance(stdin, bytes) else stdin or subprocess.DEVNULL),
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=SAFE_ENV,
                                    start_new_session=True, close_fds=True)
        except FileNotFoundError:
            if check:
                fail("missing_prerequisite", "Required command is missing: " + command)
            return Result(127)
        buffers = {"stdout": bytearray(), "stderr": bytearray()}
        truncated = False
        deadline = time.monotonic() + timeout
        try:
            if isinstance(stdin, bytes):
                # JSON credentials are capped well below a pipe buffer.
                try:
                    proc.stdin.write(stdin)
                    proc.stdin.close()
                except BrokenPipeError:
                    pass
            with selectors.DefaultSelector() as sel:
                sel.register(proc.stdout, selectors.EVENT_READ, "stdout")
                sel.register(proc.stderr, selectors.EVENT_READ, "stderr")
                while sel.get_map():
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        fail("command_timeout", "Command exceeded its deadline: " + command)
                    for key, _ in sel.select(min(remaining, 0.2)):
                        chunk = os.read(key.fileobj.fileno(), 16384)
                        if not chunk:
                            sel.unregister(key.fileobj)
                        else:
                            buf = buffers[key.data]
                            buf.extend(chunk)
                            if len(buf) > OUTPUT_LIMIT:
                                del buf[:-OUTPUT_LIMIT]
                                truncated = True
                proc.wait(timeout=max(0.01, deadline - time.monotonic()))
        except BaseException:
            with contextlib.suppress(ProcessLookupError):
                os.killpg(proc.pid, signal.SIGKILL)
            proc.wait()
            raise
        finally:
            for stream in (proc.stdin, proc.stdout, proc.stderr):
                if stream is not None and not stream.closed:
                    stream.close()
        out, err = (buffers[k].decode("utf-8", "replace") for k in ("stdout", "stderr"))
        self.diagnostics = (self.diagnostics + "\n" + command + ": " + redact(out + "\n" + err, self.secret_values))[-OUTPUT_LIMIT:]
        result = Result(proc.returncode, out, err, truncated)
        if check and result.returncode:
            fail("command_failed", "Command failed: " + command,
                 {"exit_code": result.returncode, "diagnostics": self.diagnostics})
        return result


def parse_json(result, expected=dict, allow_failure=False):
    if (result.returncode and not allow_failure) or result.truncated:
        return None
    try:
        value = json.loads(result.stdout)
        return value if isinstance(value, expected) else None
    except (ValueError, TypeError):
        return None


def validate_image(value):
    if not isinstance(value, str) or len(value) > 512 or not value.isascii():
        fail("invalid_image", "Hub image must be a pinned version or sha256 digest")
    parts = value.split("@")
    if len(parts) > 2 or (len(parts) == 2 and not DIGEST.fullmatch(parts[1])):
        fail("invalid_image", "Invalid image digest")
    base = parts[0]
    last = base.rsplit("/", 1)[-1]
    tagged = ":" in last
    repository, tag = base.rsplit(":", 1) if tagged else (base, None)
    if not re.fullmatch(r"(?:[a-z0-9]+(?:[.-][a-z0-9]+)*(?::[1-9][0-9]{0,4})?/)?[a-z0-9]+(?:[._-][a-z0-9]+)*(?:/[a-z0-9]+(?:[._-][a-z0-9]+)*)*", repository):
        fail("invalid_image", "Invalid image repository")
    if tag and (not SEMVER.fullmatch(tag) or not re.fullmatch(r"[A-Za-z0-9_.-]{1,128}", tag)):
        fail("invalid_image", "Image tags must be exact semantic versions; latest is forbidden")
    if not tag and len(parts) != 2:
        fail("invalid_image", "An exact version tag or digest is required")
    return value


def version_key(value):
    match = SEMVER.fullmatch(value or "")
    if not match:
        fail("incompatible_image", "Image must declare its semantic version")
    a, b, c, pre = match.groups()
    return (int(a), int(b), int(c), 1 if pre is None else 0,
            tuple((0, int(x)) if x.isdigit() else (1, x) for x in (pre or "").split(".")))


def browser_url(value):
    if not isinstance(value, str) or len(value) > 2048 or re.search(r"[\s\\\x00-\x1f]", value):
        return None
    try:
        u = urlsplit(value)
        if (u.scheme != "https" or u.hostname not in {"login.tailscale.com", "console.tailscale.com"}
                or u.username or u.password or u.port not in (None, 443) or u.fragment):
            return None
        return value
    except ValueError:
        return None


def extract_auth_url(value):
    if isinstance(value, dict):
        # Prefer native status JSON, never Peer URLs or a synthesized hostname.
        return browser_url(value.get("AuthURL"))
    for url in re.findall(r"https://[^\s\"'<>]+", value or ""):
        valid = browser_url(url.rstrip(".,);]"))
        if valid:
            return valid
    return None


def fqdn_from_status(value):
    name = (value.get("Self") or {}).get("DNSName", "").rstrip(".")
    if (len(name) <= 253 and name.endswith(".ts.net") and name.count(".") >= 3
            and all(re.fullmatch(r"[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?", part) for part in name.split("."))):
        return name
    return None


def validate_request(request):
    if not isinstance(request, dict) or type(request.get("protocol_version")) is not int or request["protocol_version"] != PROTOCOL:
        fail("unsupported_protocol", "protocol_version must be 1")
    action = request.get("action")
    if not isinstance(action, str) or action not in ACTIONS:
        fail("invalid_action", "Unknown deployment action")
    value = request.get("deployment_id")
    try:
        if not isinstance(value, str) or str(uuid.UUID(value)) != value:
            raise ValueError()
    except ValueError:
        fail("invalid_deployment_id", "deployment_id must be a canonical UUID")
    common = {"protocol_version", "action", "deployment_id"}
    optional = {
        "install": {"hostname", "hub_image", "image_archive", "image_archive_sha256", "install_docker", "allow_session_interruptions"},
        "update": {"hub_image", "image_archive", "image_archive_sha256", "allow_session_interruptions"},
        "backup": {"allow_session_interruptions"},
        "uninstall": {"delete_data", "allow_session_interruptions"},
        "configure_https": {"allow_session_interruptions"},
        "claim": {"username", "password", "allow_session_interruptions"},
        "logs": {"lines"},
    }.get(action, set())
    if set(request) - common - optional:
        fail("invalid_request", "Request contains unsupported fields")
    for key in ("install_docker", "allow_session_interruptions", "delete_data"):
        if key in request and type(request[key]) is not bool:
            fail("invalid_request", key + " must be a boolean")
    if "hub_image" in request:
        validate_image(request["hub_image"])
    if "hostname" in request and (not isinstance(request["hostname"], str) or not re.fullmatch(r"[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?", request["hostname"])):
        fail("invalid_hostname", "hostname must be a lowercase DNS label (1–63 characters)")
    if action == "claim":
        if not isinstance(request.get("username"), str) or not re.fullmatch(r"[A-Za-z0-9_.@-]{2,64}", request["username"]):
            fail("invalid_credentials", "Invalid owner username")
        password = request.get("password")
        if not isinstance(password, str) or not 12 <= len(password.encode("utf-8")) <= 256 or "\x00" in password:
            fail("invalid_credentials", "Owner password must contain 12–256 UTF-8 bytes without NUL")
    if "lines" in request and (type(request["lines"]) is not int or not 1 <= request["lines"] <= 500):
        fail("invalid_request", "lines must be between 1 and 500")
    if "image_archive" in request:
        value = request["image_archive"]
        if not isinstance(value, str) or len(value) > 4096 or not value.startswith("/") or any(x in value.split("/") for x in ("..", ".")) or re.search(r"[\x00-\x1f]", value):
            fail("invalid_archive", "image_archive must be an absolute regular-file path without traversal")
        digest = request.get("image_archive_sha256")
        if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
            fail("invalid_archive", "image_archive requires its authenticated image_archive_sha256 checksum")
    elif "image_archive_sha256" in request:
        fail("invalid_archive", "image_archive_sha256 requires image_archive")
    return request


def ensure_path(path, directory=False, uid=0, private=True):
    """Check EVERY component, including an existing final component, without links."""
    path = Path(path)
    for part in [*reversed(path.parents), path]:
        info = part.lstat()
        if stat.S_ISLNK(info.st_mode):
            fail("unsafe_path", "Symbolic links are forbidden in managed paths")
        if part != path or directory:
            if not stat.S_ISDIR(info.st_mode):
                fail("unsafe_path", "A managed path component is not a directory")
        elif not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
            fail("unsafe_path", "Managed files must be regular files without hard links")
        if part == path:
            if info.st_uid != uid or info.st_mode & (0o077 if private else 0o022):
                fail("ownership_mismatch", "Managed path ownership or permissions do not match")
        elif info.st_uid != 0 or info.st_mode & 0o022:
            fail("unsafe_path", "Managed parent directories must be root-owned and not writable by others")
    return path


def atomic_json(path, value):
    path = Path(path)
    if path.exists() or path.is_symlink():
        ensure_path(path)
    fd, temporary = tempfile.mkstemp(prefix=".write-", dir=path.parent)
    try:
        os.fchmod(fd, 0o600)
        with os.fdopen(fd, "w") as stream:
            json.dump(value, stream, sort_keys=True, indent=2)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        fsync_directory(path.parent)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def fsync_directory(path):
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def compose_spec(record, directory):
    """JSON is a YAML subset. The uploaded helper needs no accompanying assets."""
    directory = str(directory)
    labels = {LABEL + "manager": MANAGER, LABEL + "deployment": record["deployment_id"], LABEL + "owner": record["owner_token"]}
    logging = {"driver": "local", "options": {"max-size": "10m", "max-file": "3"}}
    common = {"init": True, "restart": "unless-stopped", "labels": labels,
              "logging": logging, "security_opt": ["no-new-privileges:true"], "cap_drop": ["ALL"]}
    tail = {**copy.deepcopy(common), "image": record["tailscale_image"], "pull_policy": "never",
            "hostname": record["hostname"], "cap_add": ["NET_ADMIN", "NET_RAW"],
            "devices": ["/dev/net/tun:/dev/net/tun:rw"],
            "environment": {"TS_HOSTNAME": record["hostname"], "TS_USERSPACE": "false", "TS_ACCEPT_DNS": "true",
                            "TS_STATE_DIR": "/var/lib/tailscale", "TS_AUTH_ONCE": "true", "TS_BOOT_TIMEOUT": "30m",
                            "TS_SOCKET": "/run/tailscale/tailscaled.sock", "TS_SERVE_CONFIG": "/config/serve.json"},
            "volumes": [{"type": "bind", "source": directory + "/tailscale-state", "target": "/var/lib/tailscale", "bind": {"create_host_path": False}},
                        {"type": "bind", "source": directory + "/serve", "target": "/config", "read_only": True, "bind": {"create_host_path": False}}],
            "tmpfs": ["/tmp:rw,nosuid,nodev,noexec,mode=1777,size=64m", "/run:rw,nosuid,nodev,noexec,mode=0755,size=16m"],
            "healthcheck": {"test": ["CMD", "tailscale", "--socket=/run/tailscale/tailscaled.sock", "status", "--json"], "interval": "30s", "timeout": "10s", "start_period": "30m", "retries": 3},
            "stop_grace_period": "30s"}
    hub = {**copy.deepcopy(common), "image": record["image_id"], "pull_policy": "never",
           "user": HUB_USER, "read_only": True, "network_mode": "service:tailscale",
           "depends_on": {"tailscale": {"condition": "service_started", "restart": True}},
           "command": ["web", "--bind", "127.0.0.1:8080"],
           "environment": {"PORTAL_HUB_STATE_DIR": "/var/lib/portal-hub", "PORTAL_HUB_RUN_DIR": "/run/portal-hub",
                           "PORTAL_HUB_REQUIRE_BOOTSTRAP": "true", "PORTAL_HUB_NETWORK_MODE": "sidecar",
                           "PORTAL_HUB_PUBLIC_URL": record.get("public_url") or "http://127.0.0.1:8080",
                           "PORTAL_HUB_MAX_LOG_BYTES": "16777216", "PORTAL_HUB_LOGGING_MODE": "full", "HOME": "/var/lib/portal-hub"},
           "volumes": [{"type": "bind", "source": directory + "/hub-data", "target": "/var/lib/portal-hub", "bind": {"create_host_path": False}}],
           "tmpfs": ["/run/portal-hub:rw,nosuid,nodev,noexec,uid=10001,gid=10001,mode=0700,size=64m", "/tmp:rw,nosuid,nodev,mode=1777,size=64m"],
           "healthcheck": {"test": ["CMD", "curl", "--noproxy", "*", "--fail", "--silent", "--max-time", "5", "http://127.0.0.1:8080/api/info"], "interval": "15s", "timeout": "8s", "start_period": "30s", "retries": 4},
           "stop_grace_period": "30s"}
    return {"name": record["project"], "services": {"tailscale": tail, "hub": hub}, "networks": {"default": {"labels": labels}}}


def private_serve_config(name):
    return {"TCP": {"443": {"HTTPS": True}}, "Web": {name + ":443": {"Handlers": {"/": {"Proxy": "http://127.0.0.1:8080"}}}}, "AllowFunnel": {}}


class Manager:
    def __init__(self, request, runner=None, root=ROOT):
        self.request = validate_request(request)
        self.action, self.id = request["action"], request["deployment_id"]
        self.root, self.directory = Path(root), Path(root) / self.id
        self.record_path = self.directory / "deployment.json"
        password = request.get("password", "")
        self.runner = runner or Runner((password, json.dumps(password)[1:-1]))
        self.record = None
        self.containers = {}

    def run(self, args, **kwargs):
        return self.runner.run(args, **kwargs)

    def load(self):
        if not self.directory.exists() and not self.directory.is_symlink():
            return False
        ensure_path(self.directory, directory=True)
        try:
            ensure_path(self.record_path)
            record = json.loads(self.record_path.read_text())
        except FileNotFoundError:
            fail("unowned_installation", "Installation directory exists without an ownership record")
        except ValueError:
            fail("invalid_record", "Deployment record is not valid JSON")
        if (record.get("manager") != MANAGER or record.get("deployment_id") != self.id or record.get("protocol_version") != PROTOCOL
                or record.get("project") != "portal-hub-" + self.id or not re.fullmatch(r"[0-9a-f]{64}", record.get("owner_token", ""))):
            fail("ownership_mismatch", "Deployment ownership record does not match")
        validate_image(record["hub_image"])
        if not DIGEST.fullmatch(record.get("image_id", "")) or record.get("tailscale_image") != TAILSCALE_IMAGE:
            fail("invalid_record", "Deployment image identity is invalid")
        if not re.fullmatch(r"[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?", record.get("hostname", "")):
            fail("invalid_record", "Invalid recorded hostname")
        if record.get("public_url"):
            name = record["public_url"].removeprefix("https://")
            if record["public_url"] != "https://" + (fqdn_from_status({"Self": {"DNSName": name}}) or ""):
                fail("invalid_record", "Invalid recorded endpoint")
        self.record = record
        for name, uid in (("hub-data", 10001), ("tailscale-state", 0), ("serve", 0), ("backups", 0)):
            path = self.directory / name
            if path.exists() or path.is_symlink():
                ensure_path(path, directory=True, uid=uid)
        return True

    def save(self):
        atomic_json(self.record_path, self.record)

    @contextlib.contextmanager
    def locked(self):
        mutation = self.action not in READ_ONLY
        if mutation and os.geteuid() != 0:
            fail("root_required", "Run this action as root through sudo")
        if mutation:
            if not self.root.exists():
                ensure_path(self.root.parent, directory=True, private=False)
                self.root.mkdir(mode=0o700)
            ensure_path(self.root, directory=True)
            locks = self.root / ".locks"
            if not locks.exists():
                locks.mkdir(mode=0o700)
            ensure_path(locks, directory=True)
            lock = locks / (self.id + ".lock")
            fd = os.open(lock, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        else:
            lock = self.root / ".locks" / (self.id + ".lock")
            if not lock.exists():
                yield
                return
            ensure_path(lock)
            fd = os.open(lock, os.O_RDONLY | os.O_NOFOLLOW)
        try:
            info = os.fstat(fd)
            if info.st_uid != 0 or info.st_mode & 0o077 or info.st_nlink != 1:
                fail("unsafe_path", "Unsafe deployment lock")
            try:
                fcntl.flock(fd, (fcntl.LOCK_EX if mutation else fcntl.LOCK_SH) | fcntl.LOCK_NB)
            except BlockingIOError:
                fail("deployment_busy", "Another action is managing this deployment; retry status shortly")
            yield
        finally:
            os.close(fd)

    def compose(self, *args, **kwargs):
        return self.run(["docker", "compose", "--project-name", self.record["project"], "--project-directory", str(self.directory),
                         "--file", str(self.directory / "compose.yaml"), *args], **kwargs)

    def write_config(self):
        atomic_json(self.directory / "compose.yaml", compose_spec(self.record, self.directory))

    def ownership(self):
        """Check actual Engine labels, never trust container names/Compose alone."""
        project = self.record["project"]
        labels = {LABEL + "manager": MANAGER, LABEL + "deployment": self.id, LABEL + "owner": self.record["owner_token"]}
        containers = {}
        for kind in ("container", "network"):
            if kind == "container":
                result = self.run(["docker", "ps", "--all", "--filter", "label=com.docker.compose.project=" + project, "--format", "{{.ID}}"])
            else:
                result = self.run(["docker", "network", "ls", "--filter", "label=com.docker.compose.project=" + project, "--format", "{{.ID}}"])
            ids = result.stdout.split()
            if result.truncated or len(ids) > 16 or any(not re.fullmatch(r"[0-9a-f]{12,64}", x) for x in ids):
                fail("ownership_mismatch", "Unexpected project resources")
            for object_id in ids:
                values = parse_json(self.run(["docker", kind, "inspect", object_id]), list)
                if not values or len(values) != 1:
                    fail("ownership_mismatch", "Cannot verify deployment resource")
                value = values[0]
                actual = value.get("Config", {}).get("Labels", {}) if kind == "container" else value.get("Labels", {})
                if any(actual.get(k) != v for k, v in labels.items()):
                    fail("ownership_mismatch", "Foreign resource uses this deployment's Compose project")
                if kind == "container":
                    service = actual.get("com.docker.compose.service")
                    if service not in {"hub", "tailscale"} or service in containers:
                        fail("ownership_mismatch", "Unexpected deployment service")
                    containers[service] = value
        self.containers = containers
        return containers

    def exec_service(self, service, *args, stdin=None, check=True, timeout=30):
        value = self.containers.get(service)
        if not value or not value.get("State", {}).get("Running"):
            return Result(125)
        user = HUB_USER if service == "hub" else "0:0"
        argv = ["docker", "exec", "--user", user]
        if stdin is not None:
            argv.append("-i")
        return self.run([*argv, value["Id"], *args], stdin=stdin, check=check, timeout=timeout)

    def hub(self, *args, **kwargs):
        return self.exec_service("hub", "portal-hub", *args, **kwargs)

    def tailscale(self, *args, **kwargs):
        return self.exec_service("tailscale", "tailscale", "--socket=/run/tailscale/tailscaled.sock", *args, **kwargs)

    def prerequisites(self):
        os_info = {}
        with contextlib.suppress(OSError):
            for line in Path("/etc/os-release").read_text().splitlines():
                key, _, value = line.partition("=")
                os_info[key] = value.strip('"\'')
        docker = self.run(["docker", "info", "--format", "{{json .ServerVersion}}"], check=False, timeout=15)
        compose = self.run(["docker", "compose", "version", "--short"], check=False, timeout=15)
        compose_version = re.match(r"v?(\d+)\.(\d+)", compose.stdout.strip())
        compose_supported = bool(compose.returncode == 0 and compose_version and tuple(map(int, compose_version.groups())) >= (2, 20))
        tun = False
        with contextlib.suppress(OSError):
            info = Path("/dev/net/tun").stat()
            tun = stat.S_ISCHR(info.st_mode) and os.major(info.st_rdev) == 10 and os.minor(info.st_rdev) == 200
        arch = {"x86_64": "amd64", "aarch64": "arm64"}.get(platform.machine(), platform.machine())
        blockers = []
        if docker.returncode:
            blockers.append("docker_unavailable")
        if not compose_supported:
            blockers.append("compose_unavailable")
        if not tun:
            blockers.append("tun_unavailable")
        if arch not in {"amd64", "arm64"}:
            blockers.append("unsupported_architecture")
        pidfd = False
        if hasattr(os, "pidfd_open"):
            with contextlib.suppress(OSError):
                fd = os.pidfd_open(os.getpid())
                os.close(fd)
                pidfd = True
        if platform.system() != "Linux" or not Path("/proc/self/status").is_file() or not pidfd:
            blockers.append("linux_pidfd_required")
        return {"docker_available": docker.returncode == 0, "compose_available": compose_supported,
                "compose_version": compose.stdout.strip() if compose.returncode == 0 else None,
                "pidfd_available": pidfd, "kernel": platform.release(),
                "tun_available": tun, "architecture": arch, "os_id": os_info.get("ID", "unknown"),
                "os_version": os_info.get("VERSION_ID"), "root": os.geteuid() == 0,
                "installed": bool(self.record and not self.record.get("uninstalled")),
                "data_retained": bool(self.record and self.record.get("uninstalled")), "blockers": blockers}

    def install_docker(self, prerequisites):
        if not self.request.get("install_docker", False):
            fail("docker_consent_required", "Docker/Compose is unavailable; explicit install_docker:true is required")
        supported = {"debian": {"13"}, "ubuntu": {"24.04"}}
        if prerequisites["os_version"] not in supported.get(prerequisites["os_id"], set()):
            fail("unsupported_docker_install", "Automatic installation supports Debian 13 and Ubuntu 24.04 distro packages; install Docker Compose v2 manually on other systems")
        # Isolate sources AND metadata, so third-party repositories, trusted=yes,
        # stale indexes, or local pinning cannot supply the privileged packages.
        if prerequisites["os_id"] == "debian":
            compose_package = "docker-compose"
            keyring = "/usr/share/keyrings/debian-archive-keyring.gpg"
            sources = "deb [signed-by=" + keyring + "] https://deb.debian.org/debian trixie main\n"
            sources += "deb [signed-by=" + keyring + "] https://deb.debian.org/debian trixie-updates main\n"
            sources += "deb [signed-by=" + keyring + "] https://security.debian.org/debian-security trixie-security main\n"
        else:
            compose_package = "docker-compose-v2"
            keyring = "/usr/share/keyrings/ubuntu-archive-keyring.gpg"
            base = "https://ports.ubuntu.com/ubuntu-ports" if prerequisites["architecture"] == "arm64" else "https://archive.ubuntu.com/ubuntu"
            security_base = base if prerequisites["architecture"] == "arm64" else "https://security.ubuntu.com/ubuntu"
            sources = "deb [signed-by=" + keyring + "] " + base + " noble main universe\n"
            sources += "deb [signed-by=" + keyring + "] " + base + " noble-updates main universe\n"
            sources += "deb [signed-by=" + keyring + "] " + security_base + " noble-security main universe\n"
        ensure_path(Path(keyring), private=False)
        with tempfile.TemporaryDirectory(prefix=".docker-packages-", dir=self.root) as working:
            work = Path(working)
            (work / "sources.list").write_text(sources)
            (work / "lists" / "partial").mkdir(parents=True, mode=0o700)
            apt = ["apt-get", "-o", "Acquire::AllowInsecureRepositories=false", "-o", "Acquire::AllowDowngradeToInsecureRepositories=false",
                   "-o", "APT::Get::AllowUnauthenticated=false", "-o", "Dir::Etc::sourcelist=" + str(work / "sources.list"),
                   "-o", "Dir::Etc::sourceparts=-", "-o", "Dir::State::lists=" + str(work / "lists"),
                   "-o", "Dir::Cache::pkgcache=" + str(work / "cache.bin"), "-o", "Dir::Cache::srcpkgcache=" + str(work / "src-cache.bin"),
                   "-o", "Dir::Etc::preferences=-", "-o", "Dir::Etc::preferencesparts=-"]
            self.run([*apt, "update"], timeout=600)
            self.run([*apt, "install", "--yes", "--no-install-recommends", "docker.io", compose_package], timeout=900)
        self.run(["systemctl", "enable", "--now", "docker"], timeout=60)

    def image_info(self, selected, archive=None, previous=None):
        validate_image(selected)
        if archive:
            self.load_archive(archive, selected)
        else:
            # Existing offline images are supported; pull only when not local.
            found = self.run(["docker", "image", "inspect", selected], check=False)
            if found.returncode:
                self.run(["docker", "pull", selected], timeout=900)
        values = parse_json(self.run(["docker", "image", "inspect", selected]), list)
        if not values or len(values) != 1 or not DIGEST.fullmatch(values[0].get("Id", "")):
            fail("invalid_image", "Cannot resolve Hub image identity")
        image = values[0]
        labels = image.get("Config", {}).get("Labels") or {}
        version = labels.get("org.opencontainers.image.version", "")
        if labels.get(LABEL + "deployment-protocol") != "1" or labels.get(LABEL + "api-version") != "2":
            fail("incompatible_image", "Hub image must declare deployment protocol 1 and HTTP API 2; use the integrated Hub Dockerfile")
        version_key(version)
        if version_key(version) < version_key("0.5.0-beta.17"):
            fail("incompatible_image", "Managed deployment requires Hub 0.5.0-beta.17 or newer")
        if previous and version_key(version) < version_key(previous["hub_version"]):
            fail("downgrade_rejected", "Hub downgrade is blocked to protect persistent data")
        host_arch = {"x86_64": "amd64", "aarch64": "arm64"}.get(platform.machine(), platform.machine())
        if image.get("Os") != "linux" or image.get("Architecture") != host_arch:
            fail("incompatible_image", "Hub image does not match this Linux host's architecture")
        digests = image.get("RepoDigests") or []
        return {"hub_image": selected, "image_id": image["Id"], "image_digest": next((x for x in digests if isinstance(x, str) and re.fullmatch(r"[a-z0-9./_:-]+@sha256:[0-9a-f]{64}", x)), None),
                "hub_version": version, "image_source": "archive" if archive else "registry_or_local"}

    def load_archive(self, name, selected):
        path = Path(name)
        # Uploaded files can be owned by the SSH user and live under /tmp, but
        # not through symlinks. Freeze once, authenticate, then canonicalize.
        parent = os.open("/", os.O_RDONLY | os.O_DIRECTORY)
        try:
            for part in path.parts[1:-1]:
                child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
                os.close(parent)
                parent = child
            fd = os.open(path.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
        except OSError:
            fail("invalid_archive", "Cannot open image archive without following links")
        finally:
            os.close(parent)
        with os.fdopen(fd, "rb") as uploaded, tempfile.TemporaryFile(dir=self.root) as stream:
            info = os.fstat(uploaded.fileno())
            if not stat.S_ISREG(info.st_mode) or not 1 <= info.st_size <= 16 * 1024**3 or info.st_nlink != 1:
                fail("invalid_archive", "Image archive must be a regular Docker-save tar up to 16 GiB")
            # Freeze the user's upload before validating it. The Docker daemon
            # must not consume bytes a nonroot uploader can subsequently change.
            remaining = info.st_size
            digest = hashlib.sha256()
            while remaining:
                chunk = uploaded.read(min(1024 * 1024, remaining))
                if not chunk:
                    fail("invalid_archive", "Image upload changed or is incomplete")
                stream.write(chunk)
                digest.update(chunk)
                remaining -= len(chunk)
            if digest.hexdigest() != self.request.get("image_archive_sha256"):
                fail("archive_integrity", "Image archive does not match the uploaded bytes; upload it again")
            stream.flush()
            stream.seek(0)
            try:
                with tarfile.open(fileobj=stream, mode="r:") as archive:
                    members = {}
                    count, total = 0, 0
                    for member in archive:
                        count += 1
                        total += member.size
                        parts = Path(member.name).parts
                        if count > 100000 or member.size < 0 or total > 16 * 1024**3 or member.name.startswith("/") or ".." in parts or not (member.isfile() or member.isdir()):
                            fail("invalid_archive", "Unsafe image archive member")
                        normalized = str(Path(member.name))
                        if normalized in members:
                            fail("invalid_archive", "Duplicate image archive member")
                        members[normalized] = member
                    def regular(name):
                        if not isinstance(name, str) or not name or name.startswith("/") or ".." in Path(name).parts:
                            fail("invalid_archive", "Unsafe Docker image reference")
                        member = members.get(str(Path(name)))
                        if not member or not member.isfile():
                            fail("invalid_archive", "Docker image references a missing regular file")
                        return member
                    metadata = regular("manifest.json")
                    if metadata.size > 1024 * 1024:
                        fail("invalid_archive", "Invalid Docker archive manifest")
                    with archive.extractfile(metadata) as entry:
                        manifest = json.load(entry)
                    if not isinstance(manifest, list) or len(manifest) != 1:
                        fail("invalid_archive", "Save exactly one tagged Hub image in the archive")
                    image = manifest[0]
                    if not isinstance(image, dict) or image.get("RepoTags") != [selected]:
                        fail("invalid_archive", "Archive must contain only the selected pinned image tag; use docker save IMAGE")
                    config = regular(image.get("Config"))
                    layers = image.get("Layers")
                    if not isinstance(layers, list) or config.size > 16 * 1024**2:
                        fail("invalid_archive", "Invalid Docker image configuration or layers")
                    layer_members = [regular(name) for name in layers]
                    if len(layers) > 100000 or config.size + sum(member.size for member in layer_members) > 16 * 1024**3:
                        fail("invalid_archive", "Referenced image content exceeds archive limits")
                    with archive.extractfile(config) as entry:
                        config_bytes = entry.read()
                    if not isinstance(json.loads(config_bytes), dict):
                        fail("invalid_archive", "Invalid Docker image configuration")
                    # Modern docker save includes BOTH manifest.json and an OCI
                    # index. containerd prefers the index, which can name other
                    # images/tags. Never forward unchecked metadata to Docker.
                    # Repack only this manifest's config/layers as a classic
                    # Docker-save graph, using fresh unambiguous member names.
                    canonical_config = hashlib.sha256(config_bytes).hexdigest() + ".json"
                    canonical_layers = ["layers/" + str(i) + "/layer.tar" for i in range(len(layers))]
                    canonical_manifest = json.dumps([{"Config": canonical_config, "RepoTags": [selected], "Layers": canonical_layers}]).encode()
                    with tempfile.TemporaryFile(dir=self.root) as canonical:
                        with tarfile.open(fileobj=canonical, mode="w:") as output:
                            for name, data in (("manifest.json", canonical_manifest), (canonical_config, config_bytes)):
                                member = tarfile.TarInfo(name)
                                member.size, member.mode = len(data), 0o600
                                output.addfile(member, io.BytesIO(data))
                            for name, source in zip(canonical_layers, layer_members):
                                member = tarfile.TarInfo(name)
                                member.size, member.mode = source.size, 0o600
                                with archive.extractfile(source) as entry:
                                    output.addfile(member, entry)
                        canonical.seek(0)
                        self.run(["docker", "load"], stdin=canonical, timeout=900)
            except (tarfile.TarError, ValueError, KeyError, TypeError):
                fail("invalid_archive", "Invalid Docker-save image archive")

    def prepare_dirs(self):
        for name, uid in (("hub-data", 10001), ("tailscale-state", 0), ("serve", 0), ("backups", 0)):
            path = self.directory / name
            if not path.exists():
                path.mkdir(mode=0o700)
                os.chown(path, uid, uid)
            ensure_path(path, directory=True, uid=uid)
        serve = self.directory / "serve" / "serve.json"
        if not serve.exists():
            atomic_json(serve, {})
        else:
            ensure_path(serve)

    def start(self, recreate=False):
        if "hub" in self.running_services():
            # Compose may replace a container for ANY configuration-hash change,
            # including one left between record save and a cancelled up call.
            # Idempotent install skips up entirely when its stack matches.
            self.maintenance_guard()
            if not self.namespace_consistent() or recreate or set(self.running_services()) != {"hub", "tailscale"}:
                recreate = True
        self.write_config()
        self.compose("config", "--quiet")
        if recreate or "hub" not in self.running_services():
            # A persisted Serve config must never race owner verification after
            # a replacement, failed bootstrap, or a restored/empty data volume.
            atomic_json(self.directory / "serve" / "serve.json", {})
        if recreate:
            # A hub shares the sidecar's network namespace. Always stop both and
            # replace both; --no-deps or replacing only tailscale is unsafe.
            self.compose("stop", "--timeout", "30", "hub", "tailscale", timeout=90)
        args = ["up", "--detach", "--no-build", "--pull", "never"]
        if recreate:
            args.append("--force-recreate")
        self.compose(*args, timeout=120)
        self.ownership()

    def ensure_images(self):
        found = self.run(["docker", "image", "inspect", self.record["image_id"]], check=False)
        if found.returncode:
            selected = self.record["hub_image"]
            archive = self.request.get("image_archive")
            if not archive and self.record.get("image_digest"):
                self.run(["docker", "pull", self.record["image_digest"]], timeout=900)
                found = self.run(["docker", "image", "inspect", self.record["image_id"]], check=False)
            if found.returncode:
                candidate = self.image_info(selected, archive)
                if candidate["image_id"] != self.record["image_id"]:
                    fail("image_identity_changed", "Recorded image content is unavailable; use update to change its selection")
        tail = self.run(["docker", "image", "inspect", self.record["tailscale_image"]], check=False)
        if tail.returncode:
            self.run(["docker", "pull", self.record["tailscale_image"]], timeout=900)

    def running_services(self):
        return [key for key, value in self.containers.items() if value.get("State", {}).get("Running")]

    def namespace_consistent(self):
        if set(self.running_services()) != {"hub", "tailscale"}:
            return False
        values = [self.exec_service(name, "readlink", "/proc/self/ns/net", check=False) for name in ("hub", "tailscale")]
        return bool(all(value.returncode == 0 and re.fullmatch(r"net:\[[0-9]+\]\s*", value.stdout) for value in values)
                    and values[0].stdout.strip() == values[1].stdout.strip())

    def configuration_current(self):
        hub = self.containers.get("hub", {})
        config = hub.get("Config", {})
        environment = dict(value.split("=", 1) for value in config.get("Env", []) if "=" in value)
        expected = compose_spec(self.record, self.directory)["services"]["hub"]["environment"]
        return bool(hub.get("Image") == self.record["image_id"]
                    and all(environment.get(key) == value for key, value in expected.items()))

    def active_sessions(self):
        if "hub" not in self.running_services():
            return 0
        value = parse_json(self.hub("list", "--active", "--format", "v1", check=False))
        if not value or not isinstance(value.get("sessions"), list):
            return None
        if any(not isinstance(item, dict) or item.get("active") is not True for item in value["sessions"]):
            return None
        return len(value["sessions"])

    def maintenance_guard(self):
        active = self.active_sessions()
        if (active is None or active > 0) and not self.request.get("allow_session_interruptions", False):
            fail("active_sessions" if active else "session_state_unknown", "Container replacement/stop terminates sessions; explicit allow_session_interruptions:true is required",
                 {"active_sessions": active})

    def wait_hub(self, seconds=45):
        deadline = time.monotonic() + seconds
        while True:
            result = parse_json(self.hub("bootstrap-status", "--json", check=False))
            api = self.local_api()
            if result is not None and type(result.get("owner_exists")) is bool and api:
                version = parse_json(self.hub("version", "--json", check=False))
                if not version or version.get("version") != self.record["hub_version"] or version.get("metadata_schema_version") != 2:
                    fail("incompatible_image", "Runtime version or metadata schema does not match the managed image contract")
                name = (self.record.get("public_url") or "").removeprefix("https://")
                config = private_serve_config(name) if result["owner_exists"] and self.record.get("https_verified") and name else {}
                operation = self.record.get("operation") or {}
                if operation.get("kind") == "update" and operation.get("phase", "candidate") in {"candidate", "rollback_restoring"}:
                    # A candidate can migrate local data while inaccessible.
                    # Never accept client writes that a retry could roll back.
                    config = {}
                atomic_json(self.directory / "serve" / "serve.json", config)
                return result
            if time.monotonic() >= deadline:
                fail("hub_not_ready", "Hub bootstrap CLI or local HTTP API is not ready; inspect doctor/logs")
            time.sleep(1)

    def local_api(self):
        value = parse_json(self.exec_service("hub", "curl", "--noproxy", "*", "--fail", "--silent", "--max-time", "5", "http://127.0.0.1:8080/api/info", check=False, timeout=8))
        return value if value and value.get("api_version") == 2 else None

    def network_status(self, timeout=30):
        return parse_json(self.tailscale("status", "--json", check=False, timeout=timeout), allow_failure=True) or {}

    def endpoint_ready(self, network, timeout=12):
        name = fqdn_from_status(network)
        if not name or self.record.get("public_url") != "https://" + name:
            return False
        ips = (network.get("Self") or {}).get("TailscaleIPs") or network.get("TailscaleIPs") or []
        address = None
        for value in ips:
            try:
                ip = ipaddress.ip_address(value)
                if ip.version == 4 and ip in ipaddress.ip_network("100.64.0.0/10"):
                    address = str(ip)
                    break
            except ValueError:
                continue
        if not address:
            return False
        value = parse_json(self.exec_service("hub", "curl", "--noproxy", "*", "--fail", "--silent", "--max-time", str(min(8, timeout)),
                                            "--resolve", name + ":443:" + address, "https://" + name + "/api/info", check=False, timeout=timeout))
        return bool(value and value.get("api_version") == 2 and value.get("public_url") == "https://" + name)

    def wait_update_https(self, seconds=30):
        # Containerboot observes the atomic Serve update asynchronously. Share
        # one deadline across status, curl and sleeps so failed probes are bounded.
        deadline = time.monotonic() + seconds
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                fail("endpoint_not_ready", "Updated Hub HTTPS endpoint failed readiness")
            try:
                network = self.network_status(timeout=min(10, remaining))
                # Login/approval remains a recoverable setup phase after an update.
                if network.get("BackendState") in {"NeedsLogin", "NeedsMachineAuth", "Stopped"}:
                    return
                remaining = deadline - time.monotonic()
                if network.get("BackendState") == "Running" and remaining > 0 and self.endpoint_ready(network, timeout=min(12, remaining)):
                    return
            except DeployError as error:
                if error.code != "command_timeout":
                    raise
                # One stalled CLI/probe consumes its allotted time, not the
                # entire readiness budget. Retry while the deadline permits.
            remaining = deadline - time.monotonic()
            if remaining > 0:
                time.sleep(min(1, remaining))

    def status(self):
        empty = {"auth_url": None, "approval_url": None, "public_url": None, "owner_exists": None, "tailscale_state": None, "active_sessions": None}
        if not self.record or self.record.get("uninstalled"):
            return self.response("not_installed", "Hub is not installed", {**empty, "data_retained": bool(self.record)})
        network = self.network_status()
        backend = network.get("BackendState")
        native_auth = extract_auth_url(network)
        owner = parse_json(self.hub("bootstrap-status", "--json", check=False))
        owner_exists = owner.get("owner_exists") if owner and type(owner.get("owner_exists")) is bool else None
        name = fqdn_from_status(network)
        url = "https://" + name if name else None
        data = {**empty, "tailscale_state": backend, "owner_exists": owner_exists,
                "public_url": url, "active_sessions": self.active_sessions(),
                "hub_image": self.record["hub_image"], "image_id": self.record["image_id"], "image_digest": self.record.get("image_digest"),
                "hub_version": self.record["hub_version"], "tailscale_image": self.record["tailscale_image"],
                "tailscale_version": network.get("Version"), "running_services": self.running_services(),
                "recovery_required": bool(self.record.get("operation")), "last_error": self.record.get("last_error")}
        local = self.local_api()
        data["application_ready"] = bool(local)
        data["network_namespace_consistent"] = self.namespace_consistent()
        data["configuration_current"] = self.configuration_current()
        serve = parse_json(self.tailscale("serve", "status", "--json", check=False))
        data["https_ready"] = bool(owner_exists and serve and serve.get("TCP", {}).get("443", {}).get("HTTPS") and not serve.get("AllowFunnel") and self.endpoint_ready(network))
        if backend == "NeedsMachineAuth":
            data["approval_url"] = native_auth or "https://login.tailscale.com/admin/machines"
            phase, message = "awaiting_approval", "Approve the Tailscale machine"
        elif backend != "Running":
            data["auth_url"] = native_auth
            phase, message = "awaiting_login", "Connect Tailscale" if native_auth else "Tailscale is starting or needs login; retry status"
        elif not name or not self.record.get("https_verified"):
            data["auth_url"] = "https://login.tailscale.com/admin/dns" if name else None
            phase, message = "awaiting_https", "Enable tailnet HTTPS and configure private Serve"
        elif owner_exists is not True:
            phase, message = "awaiting_owner", "Create the Hub owner" if owner_exists is False else "Hub bootstrap status is unavailable; inspect doctor"
        elif not data["https_ready"] or not local or not data["network_namespace_consistent"] or not data["configuration_current"]:
            phase, message = "awaiting_https", "Private HTTPS endpoint is not ready; configure HTTPS or inspect doctor"
        elif self.record.get("operation"):
            phase, message = "awaiting_https", "An interrupted maintenance operation needs recovery; retry the operation"
        else:
            phase, message = "ready", "Hub is ready"
        return self.response(phase, message, data)

    def response(self, phase, message, data=None):
        return {"protocol_version": PROTOCOL, "ok": True, "phase": phase, "message": message,
                "data": {"deployment_id": self.id, **(data or {})}}

    def recover(self):
        operation = self.record.get("operation")
        if not operation:
            return
        kind = operation["kind"]
        if kind == "update":
            phase = operation.get("phase", "candidate")
            if phase not in {"candidate", "rollback_restoring", "rollback_committed", "update_committed"}:
                fail("invalid_record", "Unknown update recovery phase")
            if phase not in {"rollback_committed", "update_committed"}:
                self.maintenance_guard()
                operation["phase"] = "rollback_restoring"
                self.save()
                atomic_json(self.directory / "serve" / "serve.json", {})
                self.compose("stop", "--timeout", "30", "hub", "tailscale", timeout=90)
                self.ownership()
                if self.running_services():
                    fail("rollback_failed", "Services must be stopped before restoring application data")
                if operation.get("snapshot_complete"):
                    self.restore_snapshot(operation["backup_path"])
                self.record.update(operation["previous"])
                # restore_snapshot fsyncs files, directories and the final rename.
                # Commit this phase BEFORE any service can resume or Serve reopen.
                # Retrying a committed rollback must never restore old data again.
                operation["phase"] = "rollback_committed"
                self.save()
            self.ownership()
            if not self.namespace_consistent() or not self.configuration_current():
                self.start(recreate=True)
            self.wait_hub()
        elif kind == "backup":
            self.resume_services(operation["running_services"])
        elif kind == "install":
            self.prepare_dirs()
            self.ensure_images()
            if set(self.running_services()) != {"hub", "tailscale"} or not self.namespace_consistent() or not self.configuration_current():
                self.start()
            self.wait_hub()
        elif kind == "uninstall":
            # Continue removal only when that action is explicitly requested.
            if self.action != "uninstall":
                fail("recovery_required", "Retry uninstall to complete an interrupted removal")
            return
        else:
            fail("invalid_record", "Unknown interrupted operation")
        self.record.pop("operation", None)
        self.record.pop("last_error", None)
        self.save()

    def resume_services(self, services):
        if services:
            self.write_config()
            self.compose("up", "--detach", "--no-build", "--pull", "never", *services, timeout=120)
            self.ownership()

    def do_install(self):
        pre = self.prerequisites()
        if not pre["docker_available"] or not pre["compose_available"]:
            self.install_docker(pre)
            pre = self.prerequisites()
        if pre["blockers"]:
            fail("prerequisites_failed", "Host prerequisites are missing", pre)
        if self.record:
            requested = self.request.get("hub_image", self.record["hub_image"])
            if requested != self.record["hub_image"] or self.request.get("hostname", self.record["hostname"]) != self.record["hostname"]:
                fail("installation_conflict", "Deployment already exists; use update to change its image")
            if self.record.get("uninstalled"):
                self.record.pop("uninstalled", None)
                self.record["operation"] = {"kind": "install"}
                self.save()
            self.prepare_dirs()
            self.ensure_images()
            if set(self.running_services()) != {"hub", "tailscale"} or not self.namespace_consistent() or not self.configuration_current():
                self.start()
            self.wait_hub()
            self.record.pop("operation", None)
            self.save()
            return self.status()
        image = self.image_info(self.request.get("hub_image", DEFAULT_IMAGE), self.request.get("image_archive"))
        self.record = {"protocol_version": PROTOCOL, "manager": MANAGER, "deployment_id": self.id,
                       "owner_token": secrets.token_hex(32), "project": "portal-hub-" + self.id,
                       "hostname": self.request.get("hostname", "portal-hub-" + self.id[:8]),
                       "tailscale_image": TAILSCALE_IMAGE, "public_url": None, "https_verified": False,
                       "created_at": int(time.time()), "operation": {"kind": "install"}, **image}
        # Publish a fully-owned directory atomically. Cancellation before this
        # rename cannot leave an unmarked final directory that blocks retry.
        with tempfile.TemporaryDirectory(prefix=".prepare-" + self.id + "-", dir=self.root) as stage:
            atomic_json(Path(stage) / "deployment.json", self.record)
            os.rename(stage, self.directory)
        self.ownership()
        self.prepare_dirs()
        self.ensure_images()
        self.start()
        self.wait_hub()
        self.record.pop("operation", None)
        self.save()
        return self.status()

    def set_public_url(self, network):
        name = fqdn_from_status(network)
        if not name:
            fail("tailscale_not_ready", "Tailscale has not returned an actual MagicDNS FQDN")
        url = "https://" + name
        if self.record.get("public_url") != url or not self.namespace_consistent() or not self.configuration_current():
            self.maintenance_guard()
            if self.record.get("public_url") != url:
                self.record["public_url"] = url
                self.record["https_verified"] = False
            self.save()
            # Changing environment requires replacing Hub. Replacing both also
            # handles a sidecar replaced outside this invocation consistently.
            self.start(recreate=True)
            self.wait_hub()
        return name

    def configure(self):
        owner = parse_json(self.hub("bootstrap-status", "--json", check=False))
        if not owner or owner.get("owner_exists") is not True:
            atomic_json(self.directory / "serve" / "serve.json", {})
        network = self.network_status()
        if network.get("BackendState") != "Running":
            return self.status()
        name = self.set_public_url(network)
        result = self.tailscale("cert", "--cert-file=/tmp/portal-hub.crt", "--key-file=/tmp/portal-hub.key", name, check=False, timeout=45)
        self.record["https_verified"] = result.returncode == 0
        self.save()
        if result.returncode:
            response = self.status()
            response.update(phase="awaiting_https", message="Enable HTTPS certificates in the tailnet, then retry")
            response["data"]["auth_url"] = extract_auth_url(result.stdout + "\n" + result.stderr) or "https://login.tailscale.com/admin/dns"
            return response
        owner = parse_json(self.hub("bootstrap-status", "--json", check=False))
        if owner and owner.get("owner_exists") is True:
            config = private_serve_config(name)
        else:
            config = {}  # Explicitly revoke any stale Serve while unclaimed.
        atomic_json(self.directory / "serve" / "serve.json", config)
        # Containerboot watches the DIRECTORY, so atomic replacement is observed.
        deadline = time.monotonic() + 15
        while owner and owner.get("owner_exists") is True and time.monotonic() < deadline:
            if self.endpoint_ready(self.network_status()):
                break
            time.sleep(1)
        return self.status()

    def do_claim(self):
        if self.network_status().get("BackendState") != "Running":
            fail("tailscale_not_ready", "Complete Tailscale enrollment before claiming the Hub")
        self.set_public_url(self.network_status())
        credentials = json.dumps({"username": self.request["username"], "password": self.request["password"]}).encode()
        result = self.hub("bootstrap-owner", "--json", stdin=credentials, check=False, timeout=60)
        owner = parse_json(result)
        if not owner or owner.get("owner_exists") is not True:
            fail("owner_claim_failed", "Owner bootstrap failed; credentials were not saved by the deployment helper")
        return self.configure()

    def do_update(self):
        self.maintenance_guard()
        selected = self.request.get("hub_image", self.record["hub_image"])
        if "hub_image" not in self.request and "image_archive" not in self.request:
            fail("invalid_request", "update requires hub_image or image_archive")
        image = self.image_info(selected, self.request.get("image_archive"), self.record)
        if image["image_id"] == self.record["image_id"]:
            if not self.namespace_consistent() or not self.configuration_current():
                self.start(recreate=True)
                self.wait_hub()
            return self.status()
        self.maintenance_guard()  # Pull/load may have taken several minutes.
        previous = {key: self.record.get(key) for key in image}
        snapshot = self.backup_path("pre-update")
        self.record["operation"] = {"kind": "update", "phase": "candidate", "previous": previous, "backup_path": str(snapshot), "snapshot_complete": False}
        self.save()
        try:
            self.compose("stop", "--timeout", "30", "hub", "tailscale", timeout=90)
            self.ownership()
            self.create_snapshot(snapshot)
            self.record["operation"]["snapshot_complete"] = True
            self.record.update(image)
            self.save()
            self.start(recreate=True)
            self.wait_hub()
            # Local readiness is checked while Serve is closed. Commit BEFORE
            # reopening the endpoint: an interrupted HTTP probe must never undo
            # sync/vault writes accepted by the newly published version.
            self.record["previous_image"] = previous
            self.record["pre_update_backup"] = str(snapshot)
            self.record["operation"]["phase"] = "update_committed"
            self.save()
            self.wait_hub()
            if self.record.get("https_verified"):
                self.wait_update_https()
        except BaseException as error:
            if isinstance(error, (KeyboardInterrupt, SystemExit)):
                raise  # Journal is left for the next invocation.
            if self.record.get("operation", {}).get("phase") == "update_committed":
                raise  # Published candidates may have accepted durable writes.
            try:
                self.recover()
            except (KeyboardInterrupt, SystemExit):
                raise
            except BaseException:
                fail("rollback_failed", "Update failed and rollback needs retry; durable data and prior image are retained")
            fail("update_rolled_back", "Update failed; the previous image was restored", {"cause": getattr(error, "code", "command_failed"), "hub_image": self.record["hub_image"]})
        self.record["previous_image"] = previous
        self.record["pre_update_backup"] = str(snapshot)
        self.record.pop("operation", None)
        self.save()
        return self.status()

    def backup_path(self, prefix="backup"):
        return self.directory / "backups" / (prefix + "-" + time.strftime("%Y%m%dT%H%M%SZ", time.gmtime()) + "-" + secrets.token_hex(4) + ".tar.gz")

    def check_no_mounts(self, directory):
        # stat/ismount misses same-filesystem bind mounts, especially files.
        # Consult the kernel mount table BEFORE walking any affected tree.
        target = Path(os.path.abspath(directory))
        try:
            lines = Path("/proc/self/mountinfo").read_text().splitlines()
            if not lines:
                raise ValueError()
            for line in lines:
                fields = line.split()
                separator = fields.index("-", 6)
                if len(fields) < separator + 4 or not fields[0].isdigit() or not fields[1].isdigit():
                    raise ValueError()
                decoded = re.sub(r"\\(040|011|012|134)", lambda m: chr(int(m[1], 8)), fields[4])
                mount = Path(decoded)
                if not mount.is_absolute():
                    raise ValueError()
                if mount == target or target in mount.parents:
                    fail("unsafe_path", "Managed data operations refuse mounted filesystems or bind-mounted files")
        except (OSError, ValueError, IndexError):
            fail("unsafe_path", "Cannot verify the kernel mount table for managed data operations")

    def create_snapshot(self, backup):
        self.check_no_mounts(self.directory)
        temporary = backup.with_suffix(".partial")
        try:
            fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
            with os.fdopen(fd, "wb") as stream:
                with tarfile.open(fileobj=stream, mode="w:gz") as archive:
                    def include(info):
                        if info.issym() or info.islnk():
                            fail("unsafe_path", "Backups refuse links inside durable data")
                        # Runtime sockets/device files are never durable state.
                        if not (info.isfile() or info.isdir()):
                            return None
                        return info
                    for name in ("hub-data", "tailscale-state", "serve", "compose.yaml", "deployment.json"):
                        archive.add(self.directory / name, arcname=name, recursive=True, filter=include)
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(temporary, backup)
            fsync_directory(backup.parent)
        finally:
            with contextlib.suppress(FileNotFoundError):
                temporary.unlink()

    def restore_snapshot(self, name):
        backup = Path(name)
        if backup.parent != self.directory / "backups" or not re.fullmatch(r"pre-update-[0-9TZ]+-[0-9a-f]{8}\.tar\.gz", backup.name):
            fail("invalid_record", "Rollback backup path does not match the owned deployment")
        self.check_no_mounts(self.directory)
        ensure_path(backup)
        # Restore ONLY application data. The same pinned Tailscale version keeps
        # its latest identity state, avoiding rollback of rotated machine keys.
        with tempfile.TemporaryDirectory(prefix=".restore-", dir=self.directory) as stage:
            stage = Path(stage)
            with tarfile.open(backup, "r:gz") as archive:
                for member in archive:
                    parts = Path(member.name).parts
                    if not parts or parts[0] != "hub-data":
                        continue
                    if member.name.startswith("/") or ".." in parts or not (member.isdir() or member.isfile()):
                        fail("invalid_backup", "Unsafe rollback archive member")
                    target = stage.joinpath(*parts)
                    if member.isdir():
                        target.mkdir(parents=True, mode=0o700, exist_ok=True)
                        os.chown(target, 10001, 10001)
                    else:
                        target.parent.mkdir(parents=True, mode=0o700, exist_ok=True)
                        fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, member.mode & 0o700 or 0o600)
                        with os.fdopen(fd, "wb") as stream, archive.extractfile(member) as source:
                            shutil.copyfileobj(source, stream)
                            stream.flush()
                            os.chown(target, 10001, 10001)
                            os.fsync(stream.fileno())
            restored = stage / "hub-data"
            if not restored.is_dir():
                fail("invalid_backup", "Rollback archive has no application data")
            for base, _, _ in os.walk(restored, topdown=False):
                os.chown(base, 10001, 10001)
                fsync_directory(base)
            current = self.directory / "hub-data"
            if current.exists():
                ensure_path(current, directory=True, uid=10001)
                shutil.rmtree(current)
            os.replace(restored, current)
            fsync_directory(stage)
            fsync_directory(self.directory)

    def do_backup(self):
        self.maintenance_guard()
        running = self.running_services()
        self.record["operation"] = {"kind": "backup", "running_services": running}
        self.save()
        backup = self.backup_path()
        try:
            self.compose("stop", "--timeout", "30", "hub", "tailscale", timeout=90)
            self.create_snapshot(backup)
        finally:
            self.resume_services(running)
        self.record.pop("operation", None)
        self.save()
        return self.response("backed_up", "Private consistent backup created; it contains vault and Tailscale identity secrets", {"backup_path": str(backup), "service_interruption": bool(running)})

    def do_uninstall(self):
        if not self.record:
            return self.response("not_installed", "Hub is not installed", {"data_retained": False})
        self.maintenance_guard()
        self.record["operation"] = {"kind": "uninstall"}
        self.save()
        self.write_config()
        self.compose("down", "--timeout", "30", timeout=120)
        self.ownership()
        if self.containers:
            fail("cleanup_failed", "Owned containers remain; retry uninstall")
        if self.request.get("delete_data", False):
            # rmtree does not follow symlinks; refuse any mount point so durable
            # data on an operator's mounted filesystem cannot be crossed.
            self.check_no_mounts(self.directory)
            shutil.rmtree(self.directory)
        else:
            self.record.pop("operation", None)
            self.record["uninstalled"] = True
            self.save()
        return self.response("not_installed", "Hub service removed", {"data_retained": not self.request.get("delete_data", False), "retained_path": None if self.request.get("delete_data") else str(self.directory)})

    def execute(self):
        with self.locked():
            try:
                return self.execute_locked()
            except (DeployError, KeyboardInterrupt, SystemExit) as error:
                if self.record and self.action not in READ_ONLY and self.directory.exists():
                    self.record["last_error"] = getattr(error, "code", "cancelled")
                    with contextlib.suppress(OSError, DeployError):
                        self.save()
                raise

    def execute_locked(self):
        self.load()
        if self.action == "inspect":
            return self.response("inspected", "Host prerequisites inspected", self.prerequisites())
        if not self.record:
            if self.action == "install":
                return self.do_install()
            if self.action in {"status", "uninstall"}:
                return self.status()
            fail("not_installed", "Deployment is not installed")
        # Every Docker exec and destructive operation is gated by labels.
        self.ownership()
        if self.action not in READ_ONLY and self.action != "uninstall":
            self.recover()
        if self.action == "install":
            return self.do_install()
        if self.action == "status":
            return self.status()
        if self.action == "configure_https":
            return self.configure()
        if self.action == "claim":
            return self.do_claim()
        if self.action == "update":
            return self.do_update()
        if self.action == "backup":
            return self.do_backup()
        if self.action == "uninstall":
            return self.do_uninstall()
        if self.action == "logs":
            chunks = []
            for name, value in self.containers.items():
                result = self.run(["docker", "logs", "--tail", str(self.request.get("lines", 200)), value["Id"]], check=False)
                chunks.append(name + ":\n" + result.stdout + "\n" + result.stderr)
            return self.response("logs", "Bounded redacted service logs", {"logs": redact("\n".join(chunks)), "limit_bytes": OUTPUT_LIMIT})
        if self.action == "doctor":
            response = self.status()
            runtime = parse_json(self.hub("doctor", "--json", check=False), allow_failure=True)
            response["data"]["checks"] = {
                "prerequisites": self.prerequisites(), "runtime": runtime,
                "application": {"ok": response["data"]["application_ready"]},
                "private_https": {"ok": response["data"]["https_ready"]},
                "network_namespace": {"ok": response["data"]["network_namespace_consistent"]},
                "configuration": {"ok": response["data"]["configuration_current"]},
                "ownership": {"ok": True}, "recovery": {"ok": not bool(self.record.get("operation"))}}
            response["data"]["healthy"] = bool(response["phase"] == "ready" and runtime and runtime.get("ok") and not self.record.get("operation"))
            return response


def cancelled(_signum, _frame):
    raise KeyboardInterrupt()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--request-stdin", action="store_true", required=True)
    parser.parse_args()
    os.umask(0o077)
    signal.signal(signal.SIGTERM, cancelled)
    signal.signal(signal.SIGINT, cancelled)
    manager = None
    secret_values = ()
    try:
        raw = sys.stdin.buffer.read(REQUEST_LIMIT + 1)
        if len(raw) > REQUEST_LIMIT:
            fail("invalid_request", "Request exceeds 16 KiB")
        try:
            request = json.loads(raw)
        except (ValueError, UnicodeError):
            fail("invalid_request", "Expected one UTF-8 JSON request")
        if isinstance(request, dict) and isinstance(request.get("password"), str):
            secret_values = (request["password"],)
        manager = Manager(request)
        response = manager.execute()
    except DeployError as error:
        response = {"protocol_version": PROTOCOL, "ok": False, "phase": "error", "message": redact(str(error), secret_values),
                    "error_code": error.code, "data": error.data}
    except (KeyboardInterrupt, SystemExit):
        response = {"protocol_version": PROTOCOL, "ok": False, "phase": "error", "error_code": "cancelled",
                    "message": "Operation interrupted; retry install or the maintenance action to recover", "data": {}}
    except (OSError, ValueError, KeyError, TypeError, AttributeError, subprocess.SubprocessError):
        # Exception strings may contain paths, credentials or remote tool output.
        response = {"protocol_version": PROTOCOL, "ok": False, "phase": "error", "error_code": "deployment_failed",
                    "message": "Deployment action failed; inspect prerequisites, status and redacted logs", "data": {}}
    print(json.dumps(response, separators=(",", ":"), ensure_ascii=False))
    return 0 if response["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
