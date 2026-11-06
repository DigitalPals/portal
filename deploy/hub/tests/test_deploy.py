"""Rootless tests: a stateful Engine boundary, never a Docker daemon/container.

The filesystem fixture simulates root UID/chown only. Files, modes, atomic
replacements, journals, locks, tar streams and symlink checks are real.
"""
import copy
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest import mock

HELPER = Path(__file__).resolve().parents[1] / "portal_hub_deploy.py"
spec = importlib.util.spec_from_file_location("portal_hub_deploy", HELPER)
d = importlib.util.module_from_spec(spec)
spec.loader.exec_module(d)
ID = "7bdf71ad-a270-440e-8ceb-590652b47293"
OLD = "sha256:" + "1" * 64
NEW = "sha256:" + "2" * 64
NEXT = "ghcr.io/digitalpals/portal-hub:0.5.0-beta.18"
FQDN = "hub.example-tail.ts.net"


def request(action, **kwargs):
    return {"protocol_version": 1, "action": action, "deployment_id": ID, **kwargs}


def image(identity=OLD, version="0.5.0-beta.17"):
    return {"Id": identity, "Architecture": {"x86_64": "amd64", "aarch64": "arm64"}.get(d.platform.machine(), d.platform.machine()),
            "Os": "linux", "RepoDigests": ["ghcr.io/digitalpals/portal-hub@" + identity],
            "Config": {"Labels": {"org.opencontainers.image.version": version, d.LABEL + "deployment-protocol": "1", d.LABEL + "api-version": "2"}}}


class Engine:
    def __init__(self, directory):
        self.directory = directory
        self.calls = []
        self.images = {d.DEFAULT_IMAGE: image(), NEXT: image(NEW, "0.5.0-beta.18"),
                       d.TAILSCALE_IMAGE: {"Id": "sha256:" + "3" * 64}}
        self.containers = {}
        self.network_exists = False
        self.owner = False
        self.sessions = []
        self.state = "NeedsLogin"
        self.auth_url = "https://login.tailscale.com/a/test-fixture"
        self.cert_enabled = False
        self.up_failures = 0
        self.foreign = False
        self.endpoint_broken = False
        self.session_unknown = False
        self.docker_available = True
        self.compose_available = True
        self.namespace_mismatch = False
        self.stdin_payloads = []
        self.loaded_archives = []
        self.offline = False
        self.endpoint_results = []
        self.endpoint_attempts = 0

    def config(self):
        return json.loads((self.directory / "compose.yaml").read_text())

    def run(self, argv, *, stdin=None, timeout=60, check=True):
        self.calls.append((list(argv), {"timeout": timeout, "check": check}))
        result = self.dispatch(argv, stdin)
        if check and result.returncode:
            d.fail("command_failed", "Fixture command failed")
        return result

    def dispatch(self, argv, stdin):
        def out(value, rc=0):
            return d.Result(rc, json.dumps(value))
        if argv[:2] == ["docker", "info"]:
            return out("29.0", 0 if self.docker_available else 1)
        if argv[:3] == ["docker", "compose", "version"]:
            return d.Result(0 if self.compose_available else 1, "2.40.0")
        if argv[:3] == ["docker", "image", "inspect"]:
            item = self.images.get(argv[3])
            if item is None:
                item = next((i for i in self.images.values() if i["Id"] == argv[3]), None)
            return out([item]) if item else d.Result(1)
        if argv[:2] == ["docker", "pull"]:
            return d.Result(1 if self.offline else 0)
        if argv[:2] == ["docker", "load"]:
            with tarfile.open(fileobj=stdin, mode="r:") as archive:
                self.loaded_archives.append({member.name: archive.extractfile(member).read() for member in archive if member.isfile()})
                entry = json.load(archive.extractfile("manifest.json"))[0]
            self.images[entry["RepoTags"][0]] = image()
            return d.Result(0, "Loaded image")
        if argv[:2] == ["docker", "ps"]:
            return d.Result(0, "\n".join(value["Id"] for value in self.containers.values()))
        if argv[:3] == ["docker", "network", "ls"]:
            return d.Result(0, "c" * 64 if self.network_exists else "")
        if argv[:3] == ["docker", "container", "inspect"]:
            value = copy.deepcopy(next(v for v in self.containers.values() if v["Id"] == argv[3]))
            if self.foreign:
                value["Config"]["Labels"][d.LABEL + "owner"] = "foreign"
            return out([value])
        if argv[:3] == ["docker", "network", "inspect"]:
            return out([{"Labels": self.config()["networks"]["default"]["labels"]}])
        if argv[:2] == ["docker", "compose"]:
            args = argv[argv.index("--file") + 2:]
            command = args[0]
            if command == "config":
                return d.Result()
            if command == "stop":
                for value in self.containers.values():
                    value["State"]["Running"] = False
                self.sessions = []
                return d.Result()
            if command == "down":
                self.containers = {}
                self.network_exists = False
                return d.Result()
            if command == "up":
                if self.up_failures:
                    self.up_failures -= 1
                    return d.Result(1)
                self.network_exists = True
                self.namespace_mismatch = False
                cfg = self.config()
                chosen = [x for x in ("tailscale", "hub") if x in args] or ["tailscale", "hub"]
                for name in chosen:
                    service = cfg["services"][name]
                    labels = {**service["labels"], "com.docker.compose.service": name}
                    self.containers[name] = {"Id": ("a" if name == "hub" else "b") * 64,
                                             "State": {"Running": True}, "Config": {"Labels": labels,
                                                 "Env": [k + "=" + v for k, v in service.get("environment", {}).items()]},
                                             "Image": service["image"]}
                return d.Result()
        if argv[:2] == ["docker", "exec"]:
            command = argv[argv.index("--user") + 2:]
            if command[0] == "-i":
                command = command[1:]
            container_id, args = command[0], command[1:]
            if args == ["readlink", "/proc/self/ns/net"]:
                return d.Result(0, "net:[1234]" if not self.namespace_mismatch or container_id.startswith("a") else "net:[5678]")
            if args[0] == "portal-hub":
                if args[1] == "bootstrap-status":
                    return out({"owner_exists": self.owner})
                if args[1] == "bootstrap-owner":
                    self.stdin_payloads.append(stdin)
                    credentials = json.loads(stdin)
                    if credentials["password"] == "reject-valid-password":
                        return d.Result(1, "bad credentials")
                    created = not self.owner
                    self.owner = True
                    return out({"owner_exists": True, "created": created})
                if args[1] == "list":
                    return d.Result(1) if self.session_unknown else out({"sessions": self.sessions})
                if args[1] == "version":
                    identity = self.containers["hub"]["Image"]
                    version = next(i["Config"]["Labels"]["org.opencontainers.image.version"] for i in self.images.values() if i["Id"] == identity)
                    return out({"version": version, "metadata_schema_version": 2, "api_version": 1})
                if args[1] == "doctor":
                    return out({"ok": True, "checks": [{"name": "runtime", "ok": True}]})
            if args[0] == "tailscale":
                args = args[2:]
                if args[0] == "status":
                    return out({"BackendState": self.state, "AuthURL": self.auth_url,
                                "Version": "1.104.4", "Self": {"DNSName": FQDN + ".", "TailscaleIPs": ["100.101.102.103"]}},
                               0 if self.state == "Running" else 1)
                if args[0] == "cert":
                    return d.Result(0) if self.cert_enabled else d.Result(1, "HTTPS not enabled https://login.tailscale.com/admin/dns")
                if args[:2] == ["serve", "status"]:
                    return out(json.loads((self.directory / "serve" / "serve.json").read_text()))
            if args[0] == "curl":
                record = json.loads((self.directory / "deployment.json").read_text())
                if args[-1].startswith("https:"):
                    self.endpoint_attempts += 1
                    serve = json.loads((self.directory / "serve" / "serve.json").read_text())
                    if not serve or self.endpoint_broken or (self.endpoint_results and not self.endpoint_results.pop(0)):
                        return d.Result(22)
                return out({"api_version": 2, "public_url": record.get("public_url") or "http://127.0.0.1:8080"})
        if argv[:2] == ["docker", "logs"]:
            return d.Result(0, 'password="top-secret" token=example tskey-auth-12345 https://login.tailscale.com/a/private\nnormal line\n')
        if argv[0] in {"apt-get", "systemctl"}:
            return d.Result()
        raise AssertionError("Unmocked command boundary: " + repr(argv))


class FilesystemCase(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="portal-hub-tests-")
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.root = self.base / "owned"
        self.root.mkdir(mode=0o700)
        self.directory = self.root / ID
        self.engine = Engine(self.directory)
        original = d.ensure_path

        def fixture_path(path, directory=False, uid=0, private=True):
            path = Path(path)
            if path != self.root and self.root not in path.parents:
                return original(path, directory=directory, uid=uid, private=private)
            for component in [*reversed(path.parents), path]:
                if component == self.root or self.root in component.parents:
                    info = component.lstat()
                    if stat.S_ISLNK(info.st_mode):
                        d.fail("unsafe_path", "symlink in fixture")
                    if component != path or directory:
                        if not stat.S_ISDIR(info.st_mode):
                            d.fail("unsafe_path", "directory required")
                    elif not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
                        d.fail("unsafe_path", "regular file required")
                    if private and info.st_mode & 0o077:
                        d.fail("ownership_mismatch", "private modes required")
            return path

        actual_fstat = os.fstat

        def root_stat(fd):
            info = list(actual_fstat(fd))
            info[4] = 0
            return os.stat_result(info)

        self.patchers = [mock.patch.object(d, "ensure_path", side_effect=fixture_path),
                         mock.patch.object(d.os, "geteuid", return_value=0),
                         mock.patch.object(d.os, "chown"),
                         mock.patch.object(d.os, "fstat", side_effect=root_stat)]
        for patcher in self.patchers:
            patcher.start()
            self.addCleanup(patcher.stop)

    def manager(self, action, **kwargs):
        manager = d.Manager(request(action, **kwargs), self.engine, root=self.root)
        def prereqs():
            return {"docker_available": self.engine.docker_available, "compose_available": self.engine.compose_available,
                    "tun_available": True, "architecture": "amd64", "os_id": "debian", "os_version": "13", "root": True,
                    "installed": bool(manager.record and not manager.record.get("uninstalled")),
                    "blockers": []}
        manager.prerequisites = prereqs
        return manager

    def run_action(self, action, **kwargs):
        return self.manager(action, **kwargs).execute()

    def install(self):
        return self.run_action("install")

    def record(self):
        return json.loads((self.directory / "deployment.json").read_text())

    def ready(self):
        self.install()
        self.engine.state = "Running"
        self.engine.cert_enabled = True
        return self.run_action("claim", username="owner", password="test-password")

    def assert_error(self, code, callback):
        with self.assertRaises(d.DeployError) as raised:
            callback()
        self.assertEqual(raised.exception.code, code)

    def mount_table(self, *mountpoints, raw=None):
        def escaped(value):
            return str(value).replace("\\", "\\134").replace(" ", "\\040").replace("\t", "\\011").replace("\n", "\\012")
        lines = ["1 0 8:1 / / rw - ext4 /dev/root rw"]
        # Same device as /, with a different filesystem root: a bind mount.
        lines += [str(i + 2) + " 1 8:1 /external " + escaped(path) + " rw shared:1 - ext4 /dev/root rw" for i, path in enumerate(mountpoints)]
        original = Path.read_text
        def read_text(path, *args, **kwargs):
            if str(path) == "/proc/self/mountinfo":
                return raw if raw is not None else "\n".join(lines) + "\n"
            return original(path, *args, **kwargs)
        return mock.patch.object(d.Path, "read_text", read_text)


class LifecycleTests(FilesystemCase):
    def test_inspect_is_read_only(self):
        result = self.run_action("inspect")
        self.assertFalse(result["data"]["installed"])
        self.assertEqual(list(self.root.iterdir()), [])

    def test_status_missing_never_creates_files(self):
        self.assertEqual(self.run_action("status")["phase"], "not_installed")
        self.assertEqual(list(self.root.iterdir()), [])

    def test_install_and_repeat_preserve_identity_and_data(self):
        response = self.install()
        self.assertEqual(response["phase"], "awaiting_login")
        self.assertEqual(response["data"]["auth_url"], self.engine.auth_url)
        initial = self.record()
        (self.directory / "hub-data" / "sentinel").write_text("durable")
        self.run_action("install", image_archive="/unused-on-idempotent-retry.tar", image_archive_sha256="0" * 64)
        self.assertEqual(self.record()["owner_token"], initial["owner_token"])
        self.assertEqual(self.record()["image_id"], OLD)
        self.assertEqual((self.directory / "hub-data" / "sentinel").read_text(), "durable")
        self.assertEqual(json.loads((self.directory / "serve" / "serve.json").read_text()), {})
        for path in [self.directory, self.directory / "hub-data", self.directory / "tailscale-state", self.directory / "deployment.json"]:
            self.assertEqual(path.stat().st_mode & 0o077, 0)

    def test_owner_and_https_states_no_unclaimed_exposure(self):
        self.install()
        self.engine.state = "NeedsMachineAuth"
        response = self.run_action("status")
        self.assertEqual(response["phase"], "awaiting_approval")
        self.assertEqual(response["data"]["approval_url"], self.engine.auth_url)
        self.engine.state = "Running"
        self.assertEqual(self.run_action("configure_https")["phase"], "awaiting_https")
        self.engine.cert_enabled = True
        self.assertEqual(self.run_action("configure_https")["phase"], "awaiting_owner")
        self.assertEqual(json.loads((self.directory / "serve" / "serve.json").read_text()), {})
        response = self.run_action("claim", username="owner", password='quotes " and $() ` not shell')
        self.assertEqual(response["phase"], "ready")
        self.assertEqual(response["data"]["public_url"], "https://" + FQDN)
        self.assertFalse(json.loads((self.directory / "serve" / "serve.json").read_text())["AllowFunnel"])
        claim_commands = [a for a, _ in self.engine.calls if "bootstrap-owner" in a]
        self.assertIn("-i", claim_commands[-1])
        self.assertEqual(claim_commands[-1][claim_commands[-1].index("--user") + 1], d.HUB_USER)
        self.assertNotIn("quotes", " ".join(claim_commands[-1]))
        self.assertEqual(json.loads(self.engine.stdin_payloads[-1])["password"], 'quotes " and $() ` not shell')
        self.assertNotIn("quotes", json.dumps(self.record()))
        self.assertEqual(self.run_action("claim", username="owner", password="test-password")["phase"], "ready")

    def test_failed_claim_keeps_serve_closed(self):
        self.install()
        self.engine.state = "Running"
        self.assert_error("owner_claim_failed", lambda: self.run_action("claim", username="owner", password="reject-valid-password"))
        self.assertEqual(json.loads((self.directory / "serve" / "serve.json").read_text()), {})

    def test_doctor_checks_application_and_tls(self):
        self.ready()
        result = self.run_action("doctor")
        self.assertTrue(result["data"]["healthy"])
        self.engine.endpoint_broken = True
        result = self.run_action("doctor")
        self.assertFalse(result["data"]["healthy"])
        self.assertEqual(result["phase"], "awaiting_https")

    def test_update_active_and_unknown_sessions_rejected_before_mutation(self):
        self.ready()
        self.engine.sessions = [{"active": True}]
        before = len(self.engine.calls)
        self.assert_error("active_sessions", lambda: self.run_action("update", hub_image=NEXT))
        self.assertFalse(any("stop" in args for args, _ in self.engine.calls[before:]))
        self.engine.session_unknown = True
        self.assert_error("session_state_unknown", lambda: self.run_action("backup"))

    def test_update_explicit_interrupt_recreates_dependents_preserves_selection(self):
        self.ready()
        owner = self.record()["owner_token"]
        self.engine.sessions = [{"active": True}]
        result = self.run_action("update", hub_image=NEXT, allow_session_interruptions=True)
        self.assertEqual(result["phase"], "ready")
        self.assertEqual(self.record()["hub_image"], NEXT)
        self.assertEqual(self.record()["previous_image"]["image_id"], OLD)
        self.assertEqual(self.record()["owner_token"], owner)
        force = [a for a, _ in self.engine.calls if "--force-recreate" in a]
        self.assertTrue(force)
        self.assertTrue(all("--no-deps" not in a for a in force))

    def test_update_waits_for_asynchronous_serve_readiness(self):
        self.ready()
        self.engine.endpoint_results = [False, True]
        self.engine.endpoint_attempts = 0
        with mock.patch.object(d.time, "sleep") as sleep:
            result = self.run_action("update", hub_image=NEXT)
        self.assertEqual(result["phase"], "ready")
        self.assertEqual(self.record()["image_id"], NEW)
        self.assertGreaterEqual(self.engine.endpoint_attempts, 2)
        sleep.assert_called_once_with(1)

    def test_permanent_https_failure_preserves_committed_candidate(self):
        self.ready()
        self.engine.endpoint_broken = True
        manager = self.manager("update", hub_image=NEXT)
        # Fake only the polling clock; no unit test waits for the 30s deadline.
        original = manager.wait_update_https
        def polling():
            with mock.patch.object(d.time, "monotonic", side_effect=range(100)), mock.patch.object(d.time, "sleep"):
                return original()
        manager.wait_update_https = polling
        with self.assertRaises(d.DeployError) as raised:
            manager.execute()
        self.assertEqual(raised.exception.code, "endpoint_not_ready")
        self.assertGreater(self.engine.endpoint_attempts, 1)
        self.assertEqual(self.record()["image_id"], NEW)
        self.assertEqual(self.record()["operation"]["phase"], "update_committed")

    def test_candidate_stays_private_until_committed_and_retries_preserve_writes(self):
        self.ready()
        database = self.directory / "hub-data" / "hub.db"
        database.write_bytes(b"before update")
        manager = self.manager("update", hub_image=NEXT)
        wait = manager.wait_hub
        phases = []
        def interrupt_published_candidate():
            result = wait()
            phase = self.record()["operation"]["phase"]
            phases.append(phase)
            config = json.loads((self.directory / "serve" / "serve.json").read_text())
            if phase == "candidate":
                self.assertEqual(config, {})
                return result
            self.assertEqual(phase, "update_committed")
            self.assertTrue(config)
            database.write_bytes(b"newly accepted sync and vault writes")
            raise KeyboardInterrupt()
        manager.wait_hub = interrupt_published_candidate
        with self.assertRaises(KeyboardInterrupt):
            manager.execute()
        self.assertEqual(phases, ["candidate", "update_committed"])
        retry = self.manager("install")
        retry.restore_snapshot = mock.Mock(side_effect=AssertionError("published data rolled back"))
        self.assertEqual(retry.execute()["phase"], "ready")
        self.assertEqual(self.record()["image_id"], NEW)
        self.assertEqual(database.read_bytes(), b"newly accepted sync and vault writes")
        self.assertNotIn("operation", self.record())

    def test_https_poll_retries_a_timed_out_probe_and_missing_status(self):
        self.ready()
        manager = self.manager("install")
        manager.load()
        manager.ownership()
        network = manager.network_status()
        with mock.patch.object(manager, "network_status", side_effect=[{}, network, network]), \
                mock.patch.object(manager, "endpoint_ready", side_effect=[d.DeployError("command_timeout", "probe timed out"), True]) as probe, \
                mock.patch.object(d.time, "sleep") as sleep:
            manager.wait_update_https()
        self.assertEqual(probe.call_count, 2)
        self.assertEqual(sleep.call_count, 2)

    def test_failed_update_rolls_back_exact_original_id_and_selection(self):
        self.ready()
        self.engine.up_failures = 1
        self.assert_error("update_rolled_back", lambda: self.run_action("update", hub_image=NEXT))
        self.assertEqual(self.record()["image_id"], OLD)
        self.assertEqual(self.record()["hub_image"], d.DEFAULT_IMAGE)
        self.assertNotIn("operation", self.record())
        self.assertEqual(self.engine.containers["hub"]["Image"], OLD)

    def test_failed_update_restores_database_snapshot_not_only_image(self):
        self.ready()
        database = self.directory / "hub-data" / "hub.db"
        database.write_bytes(b"before migration")
        manager = self.manager("update", hub_image=NEXT)
        original = manager.start
        def failed_migration(recreate=False):
            if manager.record["image_id"] == NEW:
                database.write_bytes(b"failed migration changed data")
                d.fail("hub_not_ready", "fixture migration failure")
            original(recreate)
        manager.start = failed_migration
        self.assert_error("update_rolled_back", manager.execute)
        self.assertEqual(database.read_bytes(), b"before migration")
        self.assertTrue(list((self.directory / "backups").glob("pre-update-*.tar.gz")))

    def test_rollback_retry_after_serve_reopens_preserves_new_durable_writes(self):
        self.ready()
        database = self.directory / "hub-data" / "hub.db"
        database.write_bytes(b"pre-update")
        manager = self.manager("update", hub_image=NEXT)
        start, wait = manager.start, manager.wait_hub
        def failed_candidate(recreate=False):
            if manager.record["image_id"] == NEW:
                database.write_bytes(b"bad migration")
                d.fail("hub_not_ready", "failed candidate")
            start(recreate)
        def interrupt_after_reopen():
            wait()
            self.assertEqual(self.engine.containers["hub"]["Image"], OLD)
            self.assertTrue(json.loads((self.directory / "serve" / "serve.json").read_text()))
            self.assertEqual(self.record()["operation"]["phase"], "rollback_committed")
            self.assertEqual(database.read_bytes(), b"pre-update")
            # A client can now acceptably write sync/vault data on the old Hub.
            database.write_bytes(b"new accepted sync and vault writes")
            with database.open("rb") as stream:
                os.fsync(stream.fileno())
            raise KeyboardInterrupt()
        manager.start, manager.wait_hub = failed_candidate, interrupt_after_reopen
        with self.assertRaises(KeyboardInterrupt):
            manager.execute()
        before = len(self.engine.calls)
        self.engine.sessions = [{"active": True}]
        self.assertEqual(self.run_action("install")["phase"], "ready")
        self.assertEqual(database.read_bytes(), b"new accepted sync and vault writes")
        self.assertNotIn("operation", self.record())
        self.assertFalse(any("stop" in argv or "--force-recreate" in argv for argv, _ in self.engine.calls[before:]))

    def test_rollback_recovery_at_each_durable_boundary(self):
        # Before restore, during restore, after data fsync, after journal commit,
        # and after service start. Replays before commit are safe while stopped;
        # replays after commit may only resume, never restore the snapshot again.
        for boundary in ("before_restore", "partial_restore", "restored", "committed", "started"):
            with self.subTest(boundary=boundary):
                if self.directory.exists():
                    self.run_action("uninstall", delete_data=True, allow_session_interruptions=True)
                self.ready()
                database = self.directory / "hub-data" / "hub.db"
                database.write_bytes(b"pre-update")
                candidate = self.manager("update", hub_image=NEXT)
                candidate.start = mock.Mock(side_effect=KeyboardInterrupt())
                with self.assertRaises(KeyboardInterrupt):
                    candidate.execute()
                database.write_bytes(b"candidate migration")
                recovery = self.manager("install")
                original_restore, original_start, original_save = recovery.restore_snapshot, recovery.start, recovery.save
                def restore(name):
                    self.assertFalse(recovery.running_services())
                    self.assertEqual(json.loads((self.directory / "serve" / "serve.json").read_text()), {})
                    if boundary == "before_restore":
                        raise KeyboardInterrupt()
                    if boundary == "partial_restore":
                        database.unlink()
                        raise KeyboardInterrupt()
                    original_restore(name)
                    if boundary == "restored":
                        raise KeyboardInterrupt()
                def save():
                    original_save()
                    if boundary == "committed" and recovery.record.get("operation", {}).get("phase") == "rollback_committed":
                        raise KeyboardInterrupt()
                def start(recreate=False):
                    self.assertEqual(self.record()["operation"]["phase"], "rollback_committed")
                    self.assertEqual(database.read_bytes(), b"pre-update")
                    original_start(recreate)
                    if boundary == "started":
                        raise KeyboardInterrupt()
                recovery.restore_snapshot, recovery.start, recovery.save = restore, start, save
                with self.assertRaises(KeyboardInterrupt):
                    recovery.execute()
                retry = self.manager("install")
                if boundary in {"committed", "started"}:
                    retry.restore_snapshot = mock.Mock(side_effect=AssertionError("committed rollback restored twice"))
                self.assertEqual(retry.execute()["phase"], "ready")
                self.assertEqual(database.read_bytes(), b"pre-update")
                self.assertNotIn("operation", self.record())

    def test_rollback_durability_failure_cannot_resume_services(self):
        self.ready()
        database = self.directory / "hub-data" / "hub.db"
        database.write_bytes(b"pre-update")
        candidate = self.manager("update", hub_image=NEXT)
        candidate.start = mock.Mock(side_effect=KeyboardInterrupt())
        with self.assertRaises(KeyboardInterrupt):
            candidate.execute()
        database.write_bytes(b"candidate migration")
        fsync = d.fsync_directory
        def fail_final_fsync(path):
            if Path(path) == self.directory and database.read_bytes() == b"pre-update":
                raise OSError("simulated durability failure after restore rename")
            fsync(path)
        with mock.patch.object(d, "fsync_directory", side_effect=fail_final_fsync), self.assertRaises(OSError):
            self.run_action("install")
        self.assertFalse(any(v["State"]["Running"] for v in self.engine.containers.values()))
        self.assertEqual(json.loads((self.directory / "serve" / "serve.json").read_text()), {})
        self.assertEqual(self.record()["operation"]["phase"], "rollback_restoring")
        self.assertEqual(self.run_action("install")["phase"], "ready")
        self.assertEqual(database.read_bytes(), b"pre-update")

    def test_sidecar_namespace_change_detected_and_guarded(self):
        self.ready()
        self.engine.namespace_mismatch = True
        result = self.run_action("status")
        self.assertFalse(result["data"]["network_namespace_consistent"])
        self.assertNotEqual(result["phase"], "ready")
        self.engine.sessions = [{"active": True}]
        self.assert_error("active_sessions", lambda: self.run_action("install"))
        result = self.run_action("update", hub_image=d.DEFAULT_IMAGE, allow_session_interruptions=True)
        self.assertEqual(result["phase"], "ready")
        self.assertTrue(result["data"]["network_namespace_consistent"])

    def test_backup_refuses_nested_mounts_and_symlinks(self):
        self.ready()
        (self.directory / "hub-data" / "external").symlink_to("/etc/passwd")
        self.assert_error("unsafe_path", lambda: self.run_action("backup"))
        self.assertTrue(self.engine.containers["hub"]["State"]["Running"])

    def test_mountinfo_finds_same_filesystem_directory_and_file_binds_without_walking(self):
        self.install()
        manager = self.manager("backup")
        data = self.directory / "hub-data"
        (data / "external").mkdir(mode=0o700)
        (data / "hub.db").write_bytes(b"keep")
        for mounted in (self.directory, data, data / "external", data / "hub.db",
                        data / "space tab\tnewline\nslash\\name"):
            with self.subTest(mounted=mounted), self.mount_table(mounted), \
                    mock.patch.object(d.os.path, "ismount", return_value=False), \
                    mock.patch.object(d.os, "walk", side_effect=AssertionError("walked before mount check")):
                self.assert_error("unsafe_path", lambda: manager.check_no_mounts(self.directory))
        with self.mount_table(self.root / (ID + "-sibling") / "hub-data", self.root / "elsewhere"):
            manager.check_no_mounts(self.directory)
        self.assertEqual((data / "hub.db").read_bytes(), b"keep")

    def test_mount_guard_covers_all_snapshot_inputs_restore_and_destructive_uninstall(self):
        self.ready()
        sentinel = self.directory / "hub-data" / "hub.db"
        sentinel.write_bytes(b"preserve")
        self.run_action("update", hub_image=NEXT)
        backup = self.record()["pre_update_backup"]
        for mount in (self.directory / "hub-data" / "hub.db", self.directory / "tailscale-state",
                      self.directory / "serve" / "serve.json", self.directory / "compose.yaml", Path(backup)):
            with self.subTest(mount=mount), self.mount_table(mount):
                manager = self.manager("backup")
                manager.load()
                with mock.patch.object(d.tarfile, "open", side_effect=AssertionError("tar accessed before mount check")):
                    self.assert_error("unsafe_path", lambda: manager.create_snapshot(manager.backup_path()))
                    self.assert_error("unsafe_path", lambda: manager.restore_snapshot(backup))
                self.assert_error("unsafe_path", lambda: self.run_action("uninstall", delete_data=True))
                self.assertEqual(sentinel.read_bytes(), b"preserve")

    def test_mountinfo_unavailable_or_malformed_fails_closed(self):
        manager = self.manager("backup")
        for table in ("", "not mountinfo\n", "1 0 8:1 / relative rw - ext4 /dev/root rw\n"):
            with self.subTest(table=table), self.mount_table(raw=table):
                self.assert_error("unsafe_path", lambda: manager.check_no_mounts(self.directory))

    def test_new_install_refuses_preexisting_foreign_compose_project(self):
        self.engine.containers["hub"] = {"Id": "a" * 64, "State": {"Running": True},
                                         "Config": {"Labels": {"com.docker.compose.service": "hub"}}}
        self.assert_error("ownership_mismatch", self.install)
        self.assertFalse(any("up" in a or "stop" in a or "down" in a for a, _ in self.engine.calls))

    def test_recovery_cannot_interrupt_new_sessions_without_consent(self):
        self.ready()
        manager = self.manager("update", hub_image=NEXT)
        manager.start = mock.Mock(side_effect=KeyboardInterrupt())
        with self.assertRaises(KeyboardInterrupt):
            manager.execute()
        # Simulate a candidate started by Docker just before SSH cancellation.
        self.engine.containers["hub"]["State"]["Running"] = True
        self.engine.sessions = [{"active": True}]
        self.assert_error("active_sessions", lambda: self.run_action("install"))

    def test_cancelled_update_is_journaled_and_recovered(self):
        self.ready()
        manager = self.manager("update", hub_image=NEXT)
        start = manager.start
        def cancel(recreate=False):
            if manager.record["image_id"] == NEW:
                raise KeyboardInterrupt()
            start(recreate)
        manager.start = cancel
        with self.assertRaises(KeyboardInterrupt):
            manager.execute()
        self.assertEqual(self.record()["operation"]["kind"], "update")
        self.assertTrue(self.run_action("status")["data"]["recovery_required"])
        self.assertNotEqual(self.run_action("status")["phase"], "ready")
        self.run_action("install")
        self.assertEqual(self.record()["image_id"], OLD)
        self.assertNotIn("operation", self.record())

    def test_interrupted_initial_install_can_resume(self):
        manager = self.manager("install")
        manager.start = mock.Mock(side_effect=KeyboardInterrupt())
        with self.assertRaises(KeyboardInterrupt):
            manager.execute()
        self.assertEqual(self.record()["operation"]["kind"], "install")
        result = self.run_action("install")
        self.assertEqual(result["phase"], "awaiting_login")
        self.assertNotIn("operation", self.record())

    def test_cancellation_before_ownership_record_never_publishes_unowned_directory(self):
        original = d.atomic_json
        def interrupt(path, value):
            if Path(path).name == "deployment.json":
                raise KeyboardInterrupt()
            original(path, value)
        with mock.patch.object(d, "atomic_json", side_effect=interrupt), self.assertRaises(KeyboardInterrupt):
            self.install()
        self.assertFalse(self.directory.exists())
        self.assertFalse(list(self.root.glob(".prepare-*")))
        self.assertEqual(self.install()["phase"], "awaiting_login")

    def test_install_retry_with_live_sessions_is_a_true_noop(self):
        self.ready()
        self.engine.sessions = [{"active": True}]
        before = len(self.engine.calls)
        self.assertEqual(self.run_action("install")["phase"], "ready")
        self.assertFalse(any("up" in a or "stop" in a for a, _ in self.engine.calls[before:]))

    def test_initial_install_succeeds_offline_with_exact_cached_images(self):
        self.engine.offline = True
        self.assertEqual(self.install()["phase"], "awaiting_login")
        self.assertFalse(any(argv[:2] == ["docker", "pull"] for argv, _ in self.engine.calls))
        self.assertTrue(any(argv == ["docker", "image", "inspect", d.TAILSCALE_IMAGE] for argv, _ in self.engine.calls))

    def test_initial_install_pulls_missing_pinned_sidecar(self):
        del self.engine.images[d.TAILSCALE_IMAGE]
        self.assertEqual(self.install()["phase"], "awaiting_login")
        self.assertTrue(any(argv == ["docker", "pull", d.TAILSCALE_IMAGE] for argv, _ in self.engine.calls))

    def test_cancelled_public_url_change_cannot_recreate_without_session_consent(self):
        self.install()
        self.engine.state = "Running"
        record = self.record()
        record["public_url"] = "https://" + FQDN
        d.atomic_json(self.directory / "deployment.json", record)
        self.engine.sessions = [{"active": True}]
        self.assertFalse(self.run_action("status")["data"]["configuration_current"])
        self.assert_error("active_sessions", lambda: self.run_action("install"))
        self.assert_error("active_sessions", lambda: self.run_action("configure_https"))
        self.run_action("install", allow_session_interruptions=True)
        self.assertTrue(self.run_action("status")["data"]["configuration_current"])

    def test_consistent_private_backup_and_restart(self):
        self.ready()
        (self.directory / "hub-data" / "hub.db").write_bytes(b"test database")
        (self.directory / "tailscale-state" / "tailscaled.state").write_bytes(b"test identity")
        response = self.run_action("backup")
        backup = Path(response["data"]["backup_path"])
        self.assertEqual(backup.stat().st_mode & 0o777, 0o600)
        with tarfile.open(backup) as archive:
            self.assertEqual(archive.extractfile("hub-data/hub.db").read(), b"test database")
            self.assertEqual(archive.extractfile("tailscale-state/tailscaled.state").read(), b"test identity")
            self.assertFalse(any(x.name.startswith("backups") or x.issym() for x in archive))
        self.assertTrue(all(v["State"]["Running"] for v in self.engine.containers.values()))
        self.assertNotIn("operation", self.record())

    def test_cancelled_backup_resumes_services_on_retry(self):
        self.ready()
        record = self.record()
        record["operation"] = {"kind": "backup", "running_services": ["tailscale", "hub"]}
        d.atomic_json(self.directory / "deployment.json", record)
        for value in self.engine.containers.values():
            value["State"]["Running"] = False
        self.run_action("install")
        self.assertTrue(all(v["State"]["Running"] for v in self.engine.containers.values()))
        self.assertNotIn("operation", self.record())

    def test_uninstall_retains_data_and_explicit_delete_removes_only_owned_path(self):
        self.ready()
        sentinel = self.directory / "hub-data" / "sentinel"
        sentinel.write_text("retain")
        result = self.run_action("uninstall")
        self.assertTrue(result["data"]["data_retained"])
        self.assertTrue(sentinel.exists())
        self.assertEqual(self.run_action("status")["phase"], "not_installed")
        self.assertFalse(self.engine.containers)
        sibling = self.root / "other"
        sibling.mkdir(mode=0o700)
        self.run_action("uninstall", delete_data=True)
        self.assertFalse(self.directory.exists())
        self.assertTrue(sibling.exists())

    def test_foreign_resources_are_never_stopped(self):
        self.install()
        self.engine.foreign = True
        before = len(self.engine.calls)
        self.assert_error("ownership_mismatch", lambda: self.run_action("uninstall", delete_data=True))
        self.assertFalse(any("down" in a or "stop" in a for a, _ in self.engine.calls[before:]))

    def test_unowned_directory_and_symlink_refused(self):
        self.directory.mkdir(mode=0o700)
        self.assert_error("unowned_installation", self.install)
        self.directory.rmdir()
        self.directory.symlink_to(self.base, target_is_directory=True)
        self.assert_error("unsafe_path", self.install)

    def test_lock_contention_is_nonblocking(self):
        one, two = self.manager("install"), self.manager("install")
        with one.locked():
            self.assert_error("deployment_busy", two.execute)

    def test_logs_are_bounded_and_redacted(self):
        self.install()
        logs = self.run_action("logs")["data"]["logs"]
        for secret in ("top-secret", "tskey-auth-12345", "login.tailscale.com", "token=example"):
            self.assertNotIn(secret, logs)
        self.assertIn("normal line", logs)

    def test_downgrade_is_rejected(self):
        self.ready()
        self.run_action("update", hub_image=NEXT)
        self.assert_error("downgrade_rejected", lambda: self.run_action("update", hub_image=d.DEFAULT_IMAGE))

    def test_docker_install_requires_explicit_boolean_consent(self):
        manager = self.manager("install")
        self.assert_error("docker_consent_required", lambda: manager.install_docker(manager.prerequisites()))
        self.assertFalse(any(a[0] == "apt-get" for a, _ in self.engine.calls))

    def test_docker_install_uses_isolated_authenticated_distro_sources(self):
        manager = self.manager("install", install_docker=True)
        with mock.patch.object(d, "ensure_path"):
            manager.install_docker(manager.prerequisites())
        apt = [a for a, _ in self.engine.calls if a[0] == "apt-get"]
        self.assertEqual(len(apt), 2)
        self.assertTrue(all("APT::Get::AllowUnauthenticated=false" in a for a in apt))
        self.assertTrue(all(any(x.startswith("Dir::State::lists=") for x in a) for a in apt))
        self.assertTrue(all("Dir::Etc::sourceparts=-" in a for a in apt))
        self.assertIn("docker-compose", apt[-1])
        self.assertNotIn("docker-compose-v2", apt[-1])

    def test_uploaded_docker_archive_is_loaded_via_stdin(self):
        path = self.base / "hub.tar"
        make_archive(path, [{"RepoTags": [d.DEFAULT_IMAGE], "Config": "config.json", "Layers": []}])
        response = self.run_action("install", image_archive=str(path), image_archive_sha256=hashlib.sha256(path.read_bytes()).hexdigest())
        self.assertEqual(response["phase"], "awaiting_login")
        self.assertEqual(self.record()["image_source"], "archive")
        self.assertTrue(any(a == ["docker", "load"] for a, _ in self.engine.calls))

    def test_malicious_or_multitag_archive_never_reaches_docker(self):
        for kind in ("traversal", "symlink", "multitag"):
            with self.subTest(kind=kind):
                path = self.base / (kind + ".tar")
                tags = [d.DEFAULT_IMAGE, "foreign:0.1.0"] if kind == "multitag" else [d.DEFAULT_IMAGE]
                make_archive(path, [{"RepoTags": tags}], unsafe=kind if kind != "multitag" else None)
                self.assert_error("invalid_archive", lambda: self.manager("install", image_archive=str(path), image_archive_sha256=hashlib.sha256(path.read_bytes()).hexdigest()).load_archive(str(path), d.DEFAULT_IMAGE))
        self.assertFalse(any(a == ["docker", "load"] for a, _ in self.engine.calls))

    def test_replaced_upload_is_rejected_before_docker_load(self):
        path = self.base / "hub.tar"
        make_archive(path, [{"RepoTags": [d.DEFAULT_IMAGE], "Config": "trusted.json", "Layers": []}])
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        # The replacement remains a valid, single-image tar with the same tag.
        make_archive(path, [{"RepoTags": [d.DEFAULT_IMAGE], "Config": "replaced.json", "Layers": []}])
        self.assert_error("archive_integrity", lambda: self.run_action("install", image_archive=str(path), image_archive_sha256=digest))
        self.assertFalse(any(a == ["docker", "load"] for a, _ in self.engine.calls))

    def test_unchecked_oci_index_cannot_import_foreign_images_or_tags(self):
        path = self.base / "dual-format.tar"
        selected_layer = b"\0" * 10240  # Valid empty tar layer.
        selected_config = {"architecture": "amd64", "os": "linux",
                           "rootfs": {"type": "layers", "diff_ids": ["sha256:" + hashlib.sha256(selected_layer).hexdigest()]}}
        foreign_config = json.dumps({"architecture": "amd64", "os": "linux", "rootfs": {"type": "layers", "diff_ids": []},
                                     "config": {"Env": ["ATTACK=foreign"]}}).encode()
        foreign_config_digest = hashlib.sha256(foreign_config).hexdigest()
        foreign_manifest = json.dumps({"schemaVersion": 2, "mediaType": "application/vnd.oci.image.manifest.v1+json",
                                      "config": {"mediaType": "application/vnd.oci.image.config.v1+json",
                                                 "digest": "sha256:" + foreign_config_digest, "size": len(foreign_config)}, "layers": []}).encode()
        index = {"schemaVersion": 2, "manifests": [{"mediaType": "application/vnd.oci.image.manifest.v1+json",
                  "digest": "sha256:" + hashlib.sha256(foreign_manifest).hexdigest(), "size": len(foreign_manifest),
                  "annotations": {"io.containerd.image.name": "unrelated/production:1.2.3",
                                  "org.opencontainers.image.ref.name": "unrelated/production:1.2.3"}}]}
        entries = {"oci-layout": b'{"imageLayoutVersion":"1.0.0"}', "index.json": json.dumps(index).encode(),
                   "blobs/sha256/" + foreign_config_digest: foreign_config,
                   "blobs/sha256/" + hashlib.sha256(foreign_manifest).hexdigest(): foreign_manifest,
                   "selected.json": json.dumps(selected_config).encode(), "selected-layer": selected_layer,
                   "repositories": b'{"unrelated/production":{"1.2.3":"foreign"}}'}
        make_archive(path, [{"RepoTags": [d.DEFAULT_IMAGE], "Config": "selected.json", "Layers": ["selected-layer"]}], entries=entries)
        self.run_action("install", image_archive=str(path), image_archive_sha256=hashlib.sha256(path.read_bytes()).hexdigest())
        loaded = self.engine.loaded_archives[-1]
        manifest = json.loads(loaded["manifest.json"])
        self.assertEqual(len(manifest), 1)
        self.assertEqual(manifest[0]["RepoTags"], [d.DEFAULT_IMAGE])
        self.assertEqual(json.loads(loaded[manifest[0]["Config"]]), selected_config)
        self.assertEqual([loaded[name] for name in manifest[0]["Layers"]], [selected_layer])
        self.assertEqual(set(loaded), {"manifest.json", manifest[0]["Config"], *manifest[0]["Layers"]})
        self.assertNotIn(b"unrelated/production", b"".join(loaded.values()))

    def test_native_dual_format_save_keeps_referenced_config_and_layer_bytes(self):
        path = self.base / "native-save.tar"
        layer_stream = io.BytesIO()
        with tarfile.open(fileobj=layer_stream, mode="w:") as layer:
            member = tarfile.TarInfo("native-image-file")
            member.size = 5
            layer.addfile(member, io.BytesIO(b"hello"))
        layer_bytes = layer_stream.getvalue()
        layer_digest = hashlib.sha256(layer_bytes).hexdigest()
        config = json.dumps({"architecture": "amd64", "os": "linux",
                             "rootfs": {"type": "layers", "diff_ids": ["sha256:" + layer_digest]}}).encode()
        config_digest = hashlib.sha256(config).hexdigest()
        oci_manifest = json.dumps({"schemaVersion": 2, "mediaType": "application/vnd.oci.image.manifest.v1+json",
                                  "config": {"mediaType": "application/vnd.oci.image.config.v1+json",
                                             "digest": "sha256:" + config_digest, "size": len(config)},
                                  "layers": [{"mediaType": "application/vnd.oci.image.layer.v1.tar",
                                              "digest": "sha256:" + layer_digest, "size": len(layer_bytes)}]}).encode()
        manifest_digest = hashlib.sha256(oci_manifest).hexdigest()
        index = {"schemaVersion": 2, "manifests": [{"mediaType": "application/vnd.oci.image.manifest.v1+json",
                 "digest": "sha256:" + manifest_digest, "size": len(oci_manifest),
                 "annotations": {"io.containerd.image.name": d.DEFAULT_IMAGE, "org.opencontainers.image.ref.name": d.DEFAULT_IMAGE}}]}
        entries = {"index.json": json.dumps(index).encode(), "oci-layout": b'{"imageLayoutVersion":"1.0.0"}',
                   "blobs/sha256/" + config_digest: config, "blobs/sha256/" + layer_digest: layer_bytes,
                   "blobs/sha256/" + manifest_digest: oci_manifest}
        make_archive(path, [{"RepoTags": [d.DEFAULT_IMAGE], "Config": "blobs/sha256/" + config_digest,
                             "Layers": ["blobs/sha256/" + layer_digest]}], entries=entries)
        self.assertEqual(self.run_action("install", image_archive=str(path), image_archive_sha256=hashlib.sha256(path.read_bytes()).hexdigest())["phase"], "awaiting_login")
        loaded = self.engine.loaded_archives[-1]
        graph = json.loads(loaded["manifest.json"])[0]
        self.assertEqual(loaded[graph["Config"]], config)
        self.assertEqual(loaded[graph["Layers"][0]], layer_bytes)
        self.assertNotIn("index.json", loaded)
        self.assertNotIn("oci-layout", loaded)

    def test_ambiguous_or_incomplete_docker_graph_never_reaches_engine(self):
        for case in ("duplicate", "normalized_duplicate", "missing_config", "missing_layer", "bad_manifest"):
            with self.subTest(case=case):
                path = self.base / (case + ".tar")
                manifest = [{"RepoTags": [d.DEFAULT_IMAGE], "Config": "config.json", "Layers": ["layer.tar"]}]
                entries = {}
                if case == "duplicate":
                    entries["manifest.json"] = json.dumps(manifest).encode()
                if case == "normalized_duplicate":
                    entries["./config.json"] = b"{}"
                if case == "missing_config":
                    manifest[0]["Config"] = "absent"
                if case == "missing_layer":
                    manifest[0]["Layers"] = ["absent"]
                if case == "bad_manifest":
                    manifest = ["not an image object"]
                make_archive(path, manifest, entries={"config.json": b"{}", "layer.tar": b"layer", **entries}, populate=False)
                self.assert_error("invalid_archive", lambda: self.run_action("install", image_archive=str(path), image_archive_sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
        self.assertFalse(self.engine.loaded_archives)


def make_archive(path, manifest, unsafe=None, entries=None, populate=True):
    with tarfile.open(path, "w") as archive:
        blob = json.dumps(manifest).encode()
        member = tarfile.TarInfo("manifest.json")
        member.size = len(blob)
        archive.addfile(member, io.BytesIO(blob))
        files = {}
        if populate:
            for image in manifest:
                if isinstance(image, dict):
                    if isinstance(image.get("Config"), str):
                        files[image["Config"]] = b'{"rootfs":{"type":"layers","diff_ids":[]}}'
                    for layer in image.get("Layers", []):
                        files[layer] = b"fixture layer bytes"
        files.update(entries or {})
        for name, data in files.items():
            member = tarfile.TarInfo(name)
            member.size = len(data)
            archive.addfile(member, io.BytesIO(data))
        if unsafe:
            member = tarfile.TarInfo("../escape" if unsafe == "traversal" else "symlink")
            if unsafe == "symlink":
                member.type = tarfile.SYMTYPE
                member.linkname = "/etc/shadow"
            archive.addfile(member)


class ValidationTests(unittest.TestCase):
    def test_image_validation_rejects_mutable_or_shell_inputs(self):
        for value in ("hub", "hub:latest", "hub:stable", "hub:1", "hub:1.2.3;touch", "--help", "hub:1.2.3\n", "hub:1.2.3$(id)", "hub@sha256:bad", "https://example/hub:1.2.3"):
            with self.subTest(value=value), self.assertRaises(d.DeployError):
                d.validate_image(value)
        for value in (d.DEFAULT_IMAGE, "localhost:5000/team/hub:1.2.3-test.4", "ghcr.io/team/hub@sha256:" + "a" * 64):
            self.assertEqual(d.validate_image(value), value)

    def test_strict_ids_fields_booleans_and_paths(self):
        cases = [request("install", install_docker="true"), request("uninstall", delete_data=1),
                 request("install", image_archive="/tmp/../evil"), request("install", image_archive="relative.tar"),
                 request("install", hostname="--help"), request("status", password="should-not-be-sent"),
                 request("status", deployment_id="../../etc"), request("install", hostname="hello\nworld")]
        for value in cases:
            with self.subTest(value=value), self.assertRaises(d.DeployError):
                d.validate_request(value)

    def test_auth_urls_are_native_validated_and_not_fqdns(self):
        for value in ("javascript:alert(1)", "https://login.tailscale.com.evil/a/x", "https://evil/login.tailscale.com", "https://user@login.tailscale.com/a", "https://login.tailscale.com:444/a", "https://login.tailscale.com\\@evil/a", "http://login.tailscale.com/a"):
            self.assertIsNone(d.extract_auth_url({"AuthURL": value}))
        self.assertEqual(d.extract_auth_url({"AuthURL": "https://login.tailscale.com/a/actual"}), "https://login.tailscale.com/a/actual")
        self.assertIsNone(d.fqdn_from_status({"Self": {"DNSName": "evil.net"}}))
        self.assertEqual(d.fqdn_from_status({"Self": {"DNSName": FQDN + "."}}), FQDN)

    def test_compose_security_and_standalone_template(self):
        record = {"deployment_id": ID, "owner_token": "f" * 64, "hostname": "portal-hub", "tailscale_image": d.TAILSCALE_IMAGE,
                  "project": "portal-hub-" + ID, "image_id": OLD, "public_url": None}
        compose = d.compose_spec(record, Path("/opt/portal-hub") / ID)
        tail, hub = compose["services"]["tailscale"], compose["services"]["hub"]
        self.assertEqual(hub["network_mode"], "service:tailscale")
        self.assertEqual(hub["user"], d.HUB_USER)
        self.assertEqual(hub["command"][-1], "127.0.0.1:8080")
        self.assertEqual(tail["cap_add"], ["NET_ADMIN", "NET_RAW"])
        self.assertNotIn("cap_add", hub)
        self.assertEqual(tail["environment"]["TS_AUTH_ONCE"], "true")
        self.assertEqual(tail["environment"]["TS_ACCEPT_DNS"], "true")
        for service in (tail, hub):
            self.assertNotIn("ports", service)
            self.assertNotIn("privileged", service)
            self.assertNotIn("docker.sock", json.dumps(service))
            self.assertNotIn("TS_AUTHKEY", json.dumps(service))
        self.assertNotIn("tailscale-state", json.dumps(hub))
        self.assertNotIn("tailscaled.sock", json.dumps(hub))
        self.assertNotIn("/config", json.dumps(hub))
        self.assertEqual(hub["logging"]["options"], {"max-size": "10m", "max-file": "3"})
        template = json.loads((HELPER.parent / "compose.template.yaml").read_text())
        self.assertEqual(template, compose)

    def test_version_ordering(self):
        self.assertLess(d.version_key("0.5.0-beta.9"), d.version_key("0.5.0-beta.17"))
        self.assertLess(d.version_key("0.5.0-beta.17"), d.version_key("0.5.0"))

    def test_real_path_policy_refuses_links_and_writable_ancestors(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "link"
            path.symlink_to("/etc/passwd")
            with self.assertRaises(d.DeployError):
                d.ensure_path(path)

    def test_cli_errors_are_one_json_object_without_secret_echo(self):
        for payload, code in ((b"{not json}", "invalid_request"),
                              (json.dumps(request("claim", username="owner", password="do-not-echo", extra="bad")).encode(), "invalid_request")):
            result = subprocess.run([sys.executable, str(HELPER), "--request-stdin"], input=payload, capture_output=True, check=False)
            response = json.loads(result.stdout)
            self.assertEqual(response["error_code"], code)
            self.assertEqual(result.returncode, 1)
            self.assertEqual(result.stderr, b"")
            self.assertNotIn(b"do-not-echo", result.stdout)


class RunnerTests(unittest.TestCase):
    def test_output_is_bounded_and_diagnostics_redacted(self):
        runner = d.Runner(("test-password",))
        result = runner.run([sys.executable, "-c", "import sys; sys.stdout.write('a'*200000+' password=test-password https://login.tailscale.com/a/secret')"])
        self.assertTrue(result.truncated)
        self.assertLessEqual(len(result.stdout), d.OUTPUT_LIMIT)
        self.assertLessEqual(len(runner.diagnostics), d.OUTPUT_LIMIT)
        self.assertNotIn("test-password", runner.diagnostics)
        self.assertNotIn("login.tailscale.com", runner.diagnostics)

    def test_stdin_not_argv_and_deadline_reaps_process(self):
        runner = d.Runner(("stdin-secret",))
        result = runner.run([sys.executable, "-c", "import sys; print(sys.stdin.read())"], stdin=b"stdin-secret")
        self.assertIn("stdin-secret", result.stdout)
        self.assertNotIn("stdin-secret", runner.diagnostics)
        with self.assertRaises(d.DeployError) as error:
            runner.run([sys.executable, "-c", "import time; time.sleep(5)"], timeout=0.02)
        self.assertEqual(error.exception.code, "command_timeout")

    def test_multiline_private_keys_are_redacted(self):
        value = "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\ndef\n-----END OPENSSH PRIVATE KEY-----"
        self.assertNotIn("abc", d.redact(value))


if __name__ == "__main__":
    unittest.main()
