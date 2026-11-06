#!/usr/bin/env python3
"""Exercise the actual Hub image, using a disposable lease on managed hosts.

The test requires no Tailscale account. It covers the application boundary:
no password sign-up, local-only owner passkey setup links, nonroot diagnostics,
runtime storage and durable owner setup state across container restart. Networking/enrollment is covered
by the deployment tests and separately needs a real tailnet acceptance test.
"""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import uuid


ROOT = Path(__file__).resolve().parents[1]


def run(argv, *, data=None, check=True, timeout=180):
    result = subprocess.run(argv, input=data, text=True, capture_output=True, timeout=timeout)
    if check and result.returncode:
        raise RuntimeError(f"{argv[0]} operation failed: {result.stderr[-2000:]}")
    return result


def main():
    args = [arg for arg in sys.argv[1:] if arg != "--build"]
    image = args[0] if args else "portal-hub:dev"
    build = "--build" in sys.argv[1:]
    helper = shutil.which("agent-resource")
    lease = os.environ.get("AGENT_RESOURCE_LEASE")
    if helper and not lease:
        return subprocess.call([
            helper, "run", "--task", "Portal Hub container bootstrap and restart test",
            "--ttl", "2h", "--disposable-data", "--", sys.executable, __file__,
            image, *(["--build"] if build else []),
        ])
    if not helper and os.environ.get("GITHUB_ACTIONS") != "true":
        raise RuntimeError("Run this test through agent-resource, or on a disposable GitHub Actions runner")

    project = "portal-hub-smoke-" + uuid.uuid4().hex[:12]
    compose = {
        "name": project,
        "services": {"hub": {
            "image": image, "pull_policy": "never", "init": True,
            "user": "10001:10001", "read_only": True,
            "cap_drop": ["ALL"], "security_opt": ["no-new-privileges:true"],
            "environment": {
                "PORTAL_HUB_REQUIRE_BOOTSTRAP": "true",
                "PORTAL_HUB_NETWORK_MODE": "sidecar",
                "PORTAL_HUB_RUN_DIR": "/run/portal-hub",
            },
            "volumes": ["data:/var/lib/portal-hub"],
            "tmpfs": [
                "/run/portal-hub:rw,nosuid,nodev,noexec,uid=10001,gid=10001,mode=0700,size=64m",
                "/tmp:rw,nosuid,nodev,mode=1777,size=64m",
            ],
            "stop_grace_period": "20s",
        }},
        "volumes": {"data": {}},
    }
    if build:
        compose["services"]["hub"]["build"] = {
            "context": str(ROOT), "dockerfile": "deploy/hub/Dockerfile",
        }
    with tempfile.TemporaryDirectory(prefix="portal-hub-smoke-") as directory:
        config = Path(directory) / "compose.json"
        config.write_text(json.dumps(compose))
        if helper:
            result = run([helper, "compose-up", "-f", str(config)], timeout=1800)
            project = result.stdout.strip().splitlines()[-1]
        else:
            run(["docker", "compose", "-f", str(config), "up", "-d"])
        container = None
        try:
            ids = run([
                "docker", "ps", "-aq", "--filter", "label=com.docker.compose.project=" + project,
                "--filter", "label=com.docker.compose.service=hub",
            ]).stdout.split()
            assert len(ids) == 1, "Expected exactly the owned Hub fixture"
            container = ids[0]

            def execute(*args, data=None, check=True):
                return run(["docker", "exec", "-i", container, *args], data=data, check=check)

            def ready():
                deadline = time.monotonic() + 30
                while time.monotonic() < deadline:
                    result = execute("curl", "-fsS", "http://127.0.0.1:8080/api/info", check=False)
                    if result.returncode == 0:
                        assert json.loads(result.stdout)["api_version"] == 2
                        return
                    time.sleep(0.2)
                raise AssertionError("Hub did not become ready")

            ready()
            assert execute("id", "-u").stdout.strip() == "10001"
            def post(path, body):
                return execute(
                    "curl", "-sS", "-o", "/dev/null", "-w", "%{http_code}",
                    "-H", "Content-Type: application/json", "--data-binary", "@-",
                    "http://127.0.0.1:8080" + path, data=json.dumps(body),
                ).stdout

            password = {"username": "smoke-owner", "password": "unique-smoke-password"}
            for path in ("/auth/register", "/auth/login"):
                assert post(path, password) in {"404", "405"}, "Hub must not offer password sign-up or sign-in"
            assert post("/webauthn/setup/info", {"token": "guessed-token"}) == "400"
            first = json.loads(execute("portal-hub", "owner-setup", "--json", "--username", "smoke-owner").stdout)
            assert first["mode"] == "setup" and first["username"] == "smoke-owner" and len(first["token"]) == 43
            latest = json.loads(execute("portal-hub", "owner-setup", "--json", "--username", "smoke-owner").stdout)
            assert post("/webauthn/setup/info", {"token": first["token"]}) == "400", "A new link replaces the old one"
            assert post("/webauthn/setup/info", {"token": latest["token"]}) == "200"
            # Until the passkey exists, doctor reports exactly that and nothing else.
            doctor = json.loads(execute("portal-hub", "doctor", "--json", check=False).stdout)
            assert doctor["owner_exists"] is False and doctor["owner_ready"] is False
            failing = [check["name"] for check in doctor["checks"] if not check["ok"]]
            assert failing == ["owner_bootstrap"], failing
            assert doctor["network_mode"] == "sidecar"
            execute("sh", "-c", "umask 077; printf transient > /run/portal-hub/smoke-marker")
            run(["docker", "restart", "--time", "20", container])
            ready()
            assert execute("test", "-e", "/run/portal-hub/smoke-marker", check=False).returncode != 0
            state = json.loads(execute("portal-hub", "bootstrap-status", "--json").stdout)
            assert state == {"owner_exists": False, "owner_ready": False}
            assert post("/webauthn/setup/info", {"token": latest["token"]}) == "200", "Restart must preserve the setup link"
            print("Hub image passed: nonroot, passkey-only owner setup, diagnostics, runtime tmpfs, durable restart")
        except BaseException:
            if container:
                # Logs can contain terminal output; this fixture has no real users/sessions.
                print(run(["docker", "logs", "--tail", "40", container], check=False).stderr, file=sys.stderr)
            raise
        finally:
            if not helper:
                run(["docker", "compose", "-f", str(config), "down", "--volumes"], check=False)
            # On managed hosts the outer agent-resource run owns teardown, including failure.
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
