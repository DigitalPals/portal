#!/usr/bin/env python3
"""Real management transport qualification in a leased local SSH container."""
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


def run(argv, **kwargs):
    return subprocess.run(argv, check=True, text=True, capture_output=True, timeout=600, **kwargs).stdout.strip()


def main():
    helper = shutil.which("agent-resource")
    if helper and not os.environ.get("AGENT_RESOURCE_LEASE"):
        return subprocess.call([helper, "run", "--task", "Portal real management SSH SFTP sudo tests", "--ttl", "2h", "--disposable-data", "--", sys.executable, __file__])
    if not helper and os.environ.get("GITHUB_ACTIONS") != "true":
        raise RuntimeError("Use agent-resource or a disposable GitHub Actions runner")
    project = "portal-management-test-" + uuid.uuid4().hex[:12]
    with tempfile.TemporaryDirectory(prefix="portal-management-test-") as temp:
        directory = Path(temp)
        key = directory / "id_ed25519"
        run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(key)])
        config = directory / "compose.json"
        config.write_text(json.dumps({"name": project, "services": {"ssh": {
            "build": {"context": str(ROOT / "tests/docker/managed-hub")},
            "ports": ["127.0.0.1::22"],
            "volumes": [str(key) + ".pub:/fixture/authorized_keys:ro"],
        }}}))
        container = None
        try:
            if helper:
                project = run([helper, "compose-up", "-f", str(config)]).splitlines()[-1]
            else:
                run(["docker", "compose", "-f", str(config), "up", "--build", "-d"])
            ids = run(["docker", "ps", "-aq", "--filter", "label=com.docker.compose.project=" + project]).split()
            assert len(ids) == 1
            container = ids[0]
            port = run(["docker", "port", container, "22/tcp"]).split(":")[-1]
            # Read the fixture key from the container through the independent
            # local Docker control plane, then compare the actual SSH prompt.
            fingerprint = None
            for _ in range(60):
                try:
                    fingerprint = run(["docker", "exec", container, "ssh-keygen", "-l", "-f", "/etc/ssh/ssh_host_ed25519_key.pub"]).split()[1]
                    break
                except subprocess.CalledProcessError:
                    time.sleep(0.2)
            assert fingerprint, "SSH fixture failed to initialize"
            env = dict(os.environ, PORTAL_MANAGEMENT_TEST_PORT=port, PORTAL_MANAGEMENT_TEST_KEY=str(key), PORTAL_MANAGEMENT_TEST_FINGERPRINT=fingerprint)
            subprocess.run(["cargo", "test", "--locked", "-p", "portal", "--lib", "real_management_ssh_sftp_sudo_and_jump_chain", "--", "--ignored", "--nocapture"], cwd=ROOT, env=env, check=True, timeout=900)
            print("Management transport passed: root, prompted/passwordless sudo, verified jump chain, isolated helper and SFTP")
        except BaseException:
            if container:
                result = subprocess.run(["docker", "logs", "--tail", "30", container], text=True, capture_output=True)
                print(result.stderr, file=sys.stderr)
            raise
        finally:
            if not helper:
                subprocess.run(["docker", "compose", "-f", str(config), "down", "--volumes"], check=False)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
