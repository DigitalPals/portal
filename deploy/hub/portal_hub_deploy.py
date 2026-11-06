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
# Portal and Hub share one version; Portal deploys the Hub of its own release.
DEFAULT_IMAGE = "ghcr.io/digitalpals/portal-hub:0.6.19"
# First Hub with passkey-only owners and one-time owner setup links.
MIN_HUB_VERSION = "0.6.19"
OWNER_SETUP_MODES = {"setup", "add", "reset"}
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
ACTIONS = {"inspect", "install", "status", "configure_https", "owner_setup", "doctor",
           "logs", "update", "backup", "uninstall"}
READ_ONLY = {"inspect", "status", "doctor", "logs"}
SEMVER = re.compile(r"v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*))?(?:\+[0-9A-Za-z.-]+)?\Z")
DIGEST = re.compile(r"sha256:[0-9a-f]{64}\Z")

# Automatic Docker installation. Distro packages are preferred; where a supported
# release lacks Compose 2.20+, Docker's official repository is used, verified
# by its OpenPGP key embedded below (never downloaded or trusted on first use).
DOCKER_DISTRO_APT = {("debian", "13"): "trixie", ("ubuntu", "24.04"): "noble", ("ubuntu", "26.04"): "resolute"}
DOCKER_REPO_APT = {("debian", "12"): "bookworm", ("ubuntu", "22.04"): "jammy"}
DOCKER_REPO_DNF = {"fedora": ("fedora", {"42", "43", "44"}), "rhel": ("rhel", {"9", "10"}),
                   "centos": ("centos", {"9", "10"}), "rocky": ("centos", {"9", "10"}),
                   "almalinux": ("centos", {"9", "10"})}
DOCKER_PACKAGES = ["docker-ce", "docker-ce-cli", "containerd.io", "docker-compose-plugin"]
DOCKER_APT_KEYRING = Path("/etc/apt/keyrings/portal-hub-docker.asc")
DOCKER_APT_SOURCE = Path("/etc/apt/sources.list.d/portal-hub-docker.list")
DOCKER_RPM_KEYRING = Path("/etc/pki/rpm-gpg/portal-hub-docker.asc")
DOCKER_DNF_REPO = Path("/etc/yum.repos.d/portal-hub-docker.repo")
APT_SOURCE_PARTS = Path("/etc/apt/sources.list.d")
CA_BUNDLE = Path("/etc/ssl/certs/ca-certificates.crt")
SYSTEMD_RUNTIME = Path("/run/systemd/system")
# Tailscale userspace networking (no /dev/net/tun) exposes SOCKS5 only inside
# the private Hub/sidecar network namespace; it publishes no host port.
USERSPACE_SOCKS = "127.0.0.1:1055"
# Fingerprint 9DC858229FC7DD38854AE2D88D81803C0EBFCD88; tests recompute it from these bytes.
DOCKER_APT_KEY = """-----BEGIN PGP PUBLIC KEY BLOCK-----

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
YT90qFF93M3v01BbxP+EIY2/9tiIPbrd
=0YYh
-----END PGP PUBLIC KEY BLOCK-----
"""
# Fingerprint 060A61C51B558A7F742B77AAC52FEB6B621E9F35; tests recompute it from these bytes.
DOCKER_RPM_KEY = """-----BEGIN PGP PUBLIC KEY BLOCK-----

mQINBFit5IEBEADDt86QpYKz5flnCsOyZ/fk3WwBKxfDjwHf/GIflo+4GWAXS7wJ
1PSzPsvSDATV10J44i5WQzh99q+lZvFCVRFiNhRmlmcXG+rk1QmDh3fsCCj9Q/yP
w8jn3Hx0zDtz8PIB/18ReftYJzUo34COLiHn8WiY20uGCF2pjdPgfxE+K454c4G7
gKFqVUFYgPug2CS0quaBB5b0rpFUdzTeI5RCStd27nHCpuSDCvRYAfdv+4Y1yiVh
KKdoe3Smj+RnXeVMgDxtH9FJibZ3DK7WnMN2yeob6VqXox+FvKYJCCLkbQgQmE50
uVK0uN71A1mQDcTRKQ2q3fFGlMTqJbbzr3LwnCBE6hV0a36t+DABtZTmz5O69xdJ
WGdBeePCnWVqtDb/BdEYz7hPKskcZBarygCCe2Xi7sZieoFZuq6ltPoCsdfEdfbO
+VBVKJnExqNZCcFUTEnbH4CldWROOzMS8BGUlkGpa59Sl1t0QcmWlw1EbkeMQNrN
spdR8lobcdNS9bpAJQqSHRZh3cAM9mA3Yq/bssUS/P2quRXLjJ9mIv3dky9C3udM
+q2unvnbNpPtIUly76FJ3s8g8sHeOnmYcKqNGqHq2Q3kMdA2eIbI0MqfOIo2+Xk0
rNt3ctq3g+cQiorcN3rdHPsTRSAcp+NCz1QF9TwXYtH1XV24A6QMO0+CZwARAQAB
tCtEb2NrZXIgUmVsZWFzZSAoQ0UgcnBtKSA8ZG9ja2VyQGRvY2tlci5jb20+iQI3
BBMBCgAhBQJYrep4AhsvBQsJCAcDBRUKCQgLBRYCAwEAAh4BAheAAAoJEMUv62ti
Hp816C0P/iP+1uhSa6Qq3TIc5sIFE5JHxOO6y0R97cUdAmCbEqBiJHUPNQDQaaRG
VYBm0K013Q1gcJeUJvS32gthmIvhkstw7KTodwOM8Kl11CCqZ07NPFef1b2SaJ7l
TYpyUsT9+e343ph+O4C1oUQw6flaAJe+8ATCmI/4KxfhIjD2a/Q1voR5tUIxfexC
/LZTx05gyf2mAgEWlRm/cGTStNfqDN1uoKMlV+WFuB1j2oTUuO1/dr8mL+FgZAM3
ntWFo9gQCllNV9ahYOON2gkoZoNuPUnHsf4Bj6BQJnIXbAhMk9H2sZzwUi9bgObZ
XO8+OrP4D4B9kCAKqqaQqA+O46LzO2vhN74lm/Fy6PumHuviqDBdN+HgtRPMUuao
xnuVJSvBu9sPdgT/pR1N9u/KnfAnnLtR6g+fx4mWz+ts/riB/KRHzXd+44jGKZra
IhTMfniguMJNsyEOO0AN8Tqcl0eRBxcOArcri7xu8HFvvl+e+ILymu4buusbYEVL
GBkYP5YMmScfKn+jnDVN4mWoN1Bq2yMhMGx6PA3hOvzPNsUoYy2BwDxNZyflzuAi
g59mgJm2NXtzNbSRJbMamKpQ69mzLWGdFNsRd4aH7PT7uPAURaf7B5BVp3UyjERW
5alSGnBqsZmvlRnVH5BDUhYsWZMPRQS9rRr4iGW0l+TH+O2VJ8aQ
=0Zqq
-----END PGP PUBLIC KEY BLOCK-----
"""


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


def tun_available():
    with contextlib.suppress(OSError):
        info = Path("/dev/net/tun").stat()
        return stat.S_ISCHR(info.st_mode) and os.major(info.st_rdev) == 10 and os.minor(info.st_rdev) == 200
    return False


def virtualization():
    """Container type for operator guidance (e.g. LXC nesting); never a blocker."""
    with contextlib.suppress(OSError):
        if Path("/run/systemd/container").is_file():
            return Path("/run/systemd/container").read_text().strip()[:32] or None
    with contextlib.suppress(OSError):
        environ = Path("/proc/1/environ").read_bytes().split(b"\0")
        for item in environ:
            if item.startswith(b"container="):
                return item[10:42].decode("ascii", "replace") or None
    return None


def docker_install_method(prerequisites):
    """Return (kind, codename/repository) or None for an unsupported system."""
    os_id, version = prerequisites.get("os_id"), prerequisites.get("os_version") or ""
    if prerequisites.get("architecture") not in {"amd64", "arm64"}:
        return None
    if (os_id, version) in DOCKER_DISTRO_APT:
        return ("apt_distro", DOCKER_DISTRO_APT[(os_id, version)])
    if (os_id, version) in DOCKER_REPO_APT:
        return ("apt_docker", DOCKER_REPO_APT[(os_id, version)])
    if os_id in DOCKER_REPO_DNF:
        repository, majors = DOCKER_REPO_DNF[os_id]
        if version.split(".")[0] in majors:
            return ("dnf", repository)
    return None


def archive_keyring(os_id, directory=Path("/usr/share/keyrings")):
    """The distro archive keyring as a regular file. Debian 13 ships the real
    keyring as .pgp with a .gpg compatibility symlink; links stay refused."""
    for suffix in (".pgp", ".gpg"):
        path = directory / (os_id + "-archive-keyring" + suffix)
        if path.is_file() and not path.is_symlink():
            return ensure_path(path, private=False)
    fail("missing_prerequisite", "The distribution archive keyring is missing; install " + os_id + "-archive-keyring")


def distro_sources(os_id, codename, architecture, scheme, keyring):
    if os_id == "debian":
        base, security = scheme + "://deb.debian.org/debian", scheme + "://security.debian.org/debian-security"
        components = "main"
    else:
        base = scheme + ("://ports.ubuntu.com/ubuntu-ports" if architecture == "arm64" else "://archive.ubuntu.com/ubuntu")
        security = base if architecture == "arm64" else scheme + "://security.ubuntu.com/ubuntu"
        components = "main universe"
    return "".join("deb [signed-by=" + str(keyring) + "] " + url + " " + suite + " " + components + "\n"
                   for url, suite in ((base, codename), (base, codename + "-updates"), (security, codename + "-security")))


def docker_apt_line(os_id, codename, architecture):
    return ("deb [arch=" + architecture + " signed-by=" + str(DOCKER_APT_KEYRING) + "] https://download.docker.com/linux/"
            + os_id + " " + codename + " stable\n")


def docker_repository_configured(directory):
    """An operator's own Docker source must not gain a conflicting Signed-By."""
    with contextlib.suppress(OSError):
        for path in directory.iterdir():
            if path != DOCKER_APT_SOURCE and path.is_file() and "download.docker.com" in path.read_text(errors="replace"):
                return True
    return False


def write_system_file(path, content):
    """Root-owned, world-readable package configuration, replaced atomically."""
    path = Path(path)
    if not path.parent.exists():
        path.parent.mkdir(mode=0o755, parents=True)
    ensure_path(path.parent, directory=True, private=False)
    if path.exists() or path.is_symlink():
        ensure_path(path, private=False)
    fd, temporary = tempfile.mkstemp(prefix=".portal-hub-", dir=path.parent)
    try:
        os.fchmod(fd, 0o644)
        with os.fdopen(fd, "w") as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


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
        "owner_setup": {"mode", "username", "allow_session_interruptions"},
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
    if action == "owner_setup":
        if request.get("mode") not in OWNER_SETUP_MODES:
            fail("invalid_request", "mode must be setup, add or reset")
        if "username" in request and (not isinstance(request["username"], str) or not re.fullmatch(r"[A-Za-z0-9_.@-]{2,64}", request["username"])):
            fail("invalid_request", "Invalid owner account name")
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
    userspace = record.get("tailscale_mode", "kernel") == "userspace"
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
    if userspace:
        # No TUN (e.g. unprivileged LXC): netstack needs neither the device nor
        # network administration capabilities. SOCKS5 stays inside the private
        # Hub/sidecar namespace and serves only the endpoint self-check.
        del tail["cap_add"], tail["devices"]
        tail["environment"]["TS_USERSPACE"] = "true"
        tail["environment"]["TS_SOCKS5_SERVER"] = USERSPACE_SOCKS
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
        self.runner = runner or Runner()
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
        if record.get("tailscale_mode", "kernel") not in {"kernel", "userspace"}:
            fail("invalid_record", "Invalid recorded Tailscale networking mode")
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
        # Some CLI versions exit 0 without a reachable daemon; require its version.
        server = parse_json(docker, str)
        if not server:
            docker = Result(docker.returncode or 1, docker.stdout, docker.stderr)
        compose = self.run(["docker", "compose", "version", "--short"], check=False, timeout=15)
        compose_version = re.match(r"v?(\d+)\.(\d+)", compose.stdout.strip())
        compose_supported = bool(compose.returncode == 0 and compose_version and tuple(map(int, compose_version.groups())) >= (2, 20))
        tun = tun_available()
        arch = {"x86_64": "amd64", "aarch64": "arm64"}.get(platform.machine(), platform.machine())
        blockers = []
        if docker.returncode:
            blockers.append("docker_unavailable")
        if not compose_supported:
            blockers.append("compose_unavailable")
        # A new installation chooses its mode once; an existing one keeps it.
        mode = self.record.get("tailscale_mode", "kernel") if self.record else ("kernel" if tun else "userspace")
        if mode == "kernel" and not tun:
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
                "tun_available": tun, "tailscale_mode": mode, "virtualization": virtualization(),
                "architecture": arch, "os_id": os_info.get("ID", "unknown"),
                "os_version": os_info.get("VERSION_ID"), "root": os.geteuid() == 0,
                "installed": bool(self.record and not self.record.get("uninstalled")),
                "data_retained": bool(self.record and self.record.get("uninstalled")), "blockers": blockers}

    def install_docker(self, prerequisites):
        if not self.request.get("install_docker", False):
            fail("docker_consent_required", "Docker/Compose is unavailable; explicit install_docker:true is required")
        method = docker_install_method(prerequisites)
        if not method:
            fail("unsupported_docker_install", "Automatic Docker installation supports Debian 12–13, Ubuntu 22.04/24.04/26.04, Fedora 42–44 and RHEL, Rocky, AlmaLinux or CentOS Stream 9–10; install Docker Compose 2.20+ manually on other systems")
        if not SYSTEMD_RUNTIME.is_dir():
            fail("systemd_required", "Automatic Docker installation needs systemd to start the Docker service; install Docker manually on this system")
        kind, codename = method
        if kind == "dnf":
            self.install_docker_dnf(codename, prerequisites)
        else:
            # Isolate sources AND metadata, so third-party repositories, trusted=yes,
            # stale indexes, or local pinning cannot supply the privileged packages.
            os_id = prerequisites["os_id"]
            keyring = archive_keyring(os_id)
            if not CA_BUNDLE.is_file():
                # Signed metadata makes plain HTTP safe for this bootstrap package.
                self.apt_install(distro_sources(os_id, codename, prerequisites["architecture"], "http", keyring), ["ca-certificates"])
            sources = distro_sources(os_id, codename, prerequisites["architecture"], "https", keyring)
            if kind == "apt_distro":
                # Debian 13 only recommends the CLI, which --no-install-recommends omits.
                packages = ["docker.io", "docker-cli", "docker-compose"] if os_id == "debian" else ["docker.io", "docker-compose-v2"]
            else:
                write_system_file(DOCKER_APT_KEYRING, DOCKER_APT_KEY)
                docker_line = docker_apt_line(os_id, codename, prerequisites["architecture"])
                sources += docker_line
                packages = DOCKER_PACKAGES
            self.apt_install(sources, packages)
            if kind == "apt_docker" and not docker_repository_configured(APT_SOURCE_PARTS):
                # Keep receiving Docker security updates through normal apt upgrades.
                write_system_file(DOCKER_APT_SOURCE, docker_line)
        self.run(["systemctl", "enable", "--now", "docker"], timeout=60)

    def apt_install(self, sources, packages):
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
            self.run([*apt, "install", "--yes", "--no-install-recommends", *packages], timeout=900)

    def install_docker_dnf(self, repository, prerequisites):
        major = (prerequisites.get("os_version") or "").split(".")[0]
        arch = {"amd64": "x86_64", "arm64": "aarch64"}[prerequisites["architecture"]]
        write_system_file(DOCKER_RPM_KEYRING, DOCKER_RPM_KEY)
        write_system_file(DOCKER_DNF_REPO, "\n".join([
            "[portal-hub-docker]", "name=Docker CE stable (installed by Portal Hub)",
            "baseurl=https://download.docker.com/linux/" + repository + "/" + major + "/" + arch + "/stable",
            "enabled=1", "gpgcheck=1", "gpgkey=file://" + str(DOCKER_RPM_KEYRING), ""]))
        # Existing podman-docker/runc conflicts fail visibly; nothing is erased.
        self.run(["dnf", "install", "--assumeyes", "--setopt=install_weak_deps=False", *DOCKER_PACKAGES], timeout=1200)

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
        if version_key(version) < version_key(MIN_HUB_VERSION):
            fail("incompatible_image", "Managed deployment requires Hub " + MIN_HUB_VERSION + " or newer")
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
        # Userspace netstack has no interface address in this namespace; reach
        # the node's own Serve listener through its private SOCKS5 server.
        proxy = (["--proxy", "socks5://" + USERSPACE_SOCKS] if self.record.get("tailscale_mode") == "userspace"
                 else ["--noproxy", "*"])
        value = parse_json(self.exec_service("hub", "curl", *proxy, "--fail", "--silent", "--max-time", str(min(8, timeout)),
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
        empty = {"auth_url": None, "approval_url": None, "public_url": None, "owner_exists": None, "owner_ready": None, "tailscale_state": None, "active_sessions": None}
        if not self.record or self.record.get("uninstalled"):
            return self.response("not_installed", "Hub is not installed", {**empty, "data_retained": bool(self.record)})
        network = self.network_status()
        backend = network.get("BackendState")
        native_auth = extract_auth_url(network)
        owner = parse_json(self.hub("bootstrap-status", "--json", check=False))
        owner_exists = owner.get("owner_exists") if owner and type(owner.get("owner_exists")) is bool else None
        owner_ready = owner.get("owner_ready") if owner and type(owner.get("owner_ready")) is bool else None
        name = fqdn_from_status(network)
        url = "https://" + name if name else None
        data = {**empty, "tailscale_state": backend, "owner_exists": owner_exists, "owner_ready": owner_ready,
                "public_url": url, "active_sessions": self.active_sessions(),
                "hub_image": self.record["hub_image"], "image_id": self.record["image_id"], "image_digest": self.record.get("image_digest"),
                "hub_version": self.record["hub_version"], "tailscale_image": self.record["tailscale_image"],
                "tailscale_mode": self.record.get("tailscale_mode", "kernel"),
                "tailscale_version": network.get("Version"), "running_services": self.running_services(),
                "recovery_required": bool(self.record.get("operation")), "last_error": self.record.get("last_error")}
        local = self.local_api()
        data["application_ready"] = bool(local)
        data["network_namespace_consistent"] = self.namespace_consistent()
        data["configuration_current"] = self.configuration_current()
        serve = parse_json(self.tailscale("serve", "status", "--json", check=False))
        # Serve is safe before the owner exists: creating one needs a setup
        # link that only the server administrator can mint.
        data["https_ready"] = bool(serve and serve.get("TCP", {}).get("443", {}).get("HTTPS") and not serve.get("AllowFunnel") and self.endpoint_ready(network))
        if backend == "NeedsMachineAuth":
            data["approval_url"] = native_auth or "https://login.tailscale.com/admin/machines"
            phase, message = "awaiting_approval", "Approve the Tailscale machine"
        elif backend != "Running":
            data["auth_url"] = native_auth
            phase, message = "awaiting_login", "Connect Tailscale" if native_auth else "Tailscale is starting or needs login; retry status"
        elif not name or not self.record.get("https_verified"):
            data["auth_url"] = "https://login.tailscale.com/admin/dns" if name else None
            phase, message = "awaiting_https", "Enable tailnet HTTPS and configure private Serve"
        elif not data["https_ready"] or not local or not data["network_namespace_consistent"] or not data["configuration_current"]:
            phase, message = "awaiting_https", "Private HTTPS endpoint is not ready; configure HTTPS or inspect doctor"
        elif self.record.get("operation"):
            phase, message = "awaiting_https", "An interrupted maintenance operation needs recovery; retry the operation"
        elif owner_ready is not True:
            # Passkeys belong to the HTTPS name, so the owner comes last.
            phase, message = ("awaiting_owner", "Create the owner passkey") if owner is not None else ("awaiting_owner", "Hub owner status is unavailable; inspect doctor")
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
                       "tailscale_image": TAILSCALE_IMAGE, "tailscale_mode": pre["tailscale_mode"],
                       "public_url": None, "https_verified": False,
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
        atomic_json(self.directory / "serve" / "serve.json", private_serve_config(name))
        # Containerboot watches the DIRECTORY, so atomic replacement is observed.
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if self.endpoint_ready(self.network_status()):
                break
            time.sleep(1)
        return self.status()

    def do_owner_setup(self):
        """Mint a one-time owner passkey link. The token goes only to stdout of
        this SSH session; the Hub stores its hash and accepts it once."""
        network = self.network_status()
        if network.get("BackendState") != "Running":
            fail("tailscale_not_ready", "Complete Tailscale enrollment before creating the owner passkey")
        self.set_public_url(network)
        response = self.status()
        if not response["data"].get("https_ready"):
            fail("https_not_ready", "Finish private HTTPS before creating the owner passkey")
        args = ["owner-setup", "--json", "--mode", self.request["mode"]]
        if "username" in self.request:
            args += ["--username", self.request["username"]]
        result = self.hub(*args, check=False, timeout=60)
        setup = parse_json(result, allow_failure=True)
        token = setup.get("token") if setup else None
        if result.returncode or not isinstance(token, str) or not re.fullmatch(r"[A-Za-z0-9_-]{43}", token):
            reason = redact(result.stderr.strip().splitlines()[-1] if result.stderr.strip() else "")[:300]
            fail("owner_setup_failed", "The Hub could not create a passkey setup link" + (": " + reason if reason else ""))
        response["data"]["owner_setup"] = {"token": token, "mode": setup.get("mode"), "username": setup.get("username"),
                                           "expires_at": setup.get("expires_at")}
        return response

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
        if self.action == "owner_setup":
            return self.do_owner_setup()
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
