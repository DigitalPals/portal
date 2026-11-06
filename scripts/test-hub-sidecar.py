#!/usr/bin/env python3
"""Exercise the pinned sidecar, kernel networking and private Hub namespace.

No tailnet credentials are supplied and the fixture never completes enrollment.
Live tailnet certificate consent remains a separate acceptance test.
"""
import importlib.util
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
spec = importlib.util.spec_from_file_location("deployment", ROOT / "deploy/hub/portal_hub_deploy.py")
deployment = importlib.util.module_from_spec(spec)
spec.loader.exec_module(deployment)


def run(argv, *, check=True, timeout=180):
    result = subprocess.run(argv, text=True, capture_output=True, timeout=timeout)
    if check and result.returncode:
        # Never expose native login links or the unauthenticated node state.
        raise RuntimeError(f"{argv[0]} operation failed with exit {result.returncode}")
    return result


def main():
    image = sys.argv[1] if len(sys.argv) > 1 else "portal-hub:dev"
    helper = shutil.which("agent-resource")
    if helper and not os.environ.get("AGENT_RESOURCE_LEASE"):
        return subprocess.call([helper, "run", "--task", "Portal pinned Tailscale sidecar kernel networking test", "--ttl", "1h", "--disposable-data", "--", sys.executable, __file__, image])
    if not helper and os.environ.get("GITHUB_ACTIONS") != "true":
        raise RuntimeError("Use agent-resource or a disposable GitHub Actions runner")
    identifier = str(uuid.uuid4())
    record = {"deployment_id": identifier, "owner_token": "f" * 64, "project": "portal-sidecar-" + identifier, "hostname": "portal-sidecar-test", "tailscale_image": deployment.TAILSCALE_IMAGE, "image_id": image, "public_url": None}
    with tempfile.TemporaryDirectory(prefix="portal-sidecar-test-") as temp:
        directory = Path(temp)
        serve = directory / "serve"
        serve.mkdir(mode=0o755)
        (serve / "serve.json").write_text("{}")
        compose = deployment.compose_spec(record, directory)
        compose["services"]["tailscale"]["volumes"] = ["identity:/var/lib/tailscale", {"type": "bind", "source": str(serve), "target": "/config", "read_only": True}]
        compose["services"]["hub"]["volumes"] = ["data:/var/lib/portal-hub"]
        compose["volumes"] = {"identity": {}, "data": {}}
        config = directory / "compose.json"
        config.write_text(json.dumps(compose))
        project = compose["name"]
        found = run(["docker", "image", "inspect", deployment.TAILSCALE_IMAGE], check=False)
        if found.returncode:
            run(["docker", "pull", deployment.TAILSCALE_IMAGE], timeout=600)
        try:
            if helper:
                project = run([helper, "compose-up", "-f", str(config)], timeout=600).stdout.strip().splitlines()[-1]
            else:
                run(["docker", "compose", "-f", str(config), "up", "-d"])
            def container(service):
                ids = run(["docker", "ps", "-aq", "--filter", "label=com.docker.compose.project=" + project, "--filter", "label=com.docker.compose.service=" + service]).stdout.split()
                assert len(ids) == 1
                return ids[0]
            tail, hub = container("tailscale"), container("hub")
            def execute(which, *args, check=True):
                return run(["docker", "exec", which, *args], check=check)
            deadline = time.monotonic() + 60
            while True:
                state = execute(tail, "tailscale", "--socket=/run/tailscale/tailscaled.sock", "status", "--json", check=False)
                try:
                    status = json.loads(state.stdout)
                except ValueError:
                    status = {}
                if status.get("BackendState") == "NeedsLogin":
                    break
                assert time.monotonic() < deadline, "Sidecar never reached native NeedsLogin state"
                time.sleep(0.25)
            assert execute(tail, "test", "-e", "/sys/class/net/tailscale0").returncode == 0
            # The pinned image uses iptables-legacy: this needs NET_RAW as well
            # as NET_ADMIN. Exercise real kernel rules inside its own namespace.
            execute(tail, "iptables", "-w", "-t", "filter", "-N", "portal-smoke")
            execute(tail, "iptables", "-w", "-t", "filter", "-A", "portal-smoke", "-j", "RETURN")
            execute(tail, "iptables", "-w", "-t", "filter", "-F", "portal-smoke")
            execute(tail, "iptables", "-w", "-t", "filter", "-X", "portal-smoke")
            assert execute(tail, "readlink", "/proc/self/ns/net").stdout == execute(hub, "readlink", "/proc/self/ns/net").stdout
            info = json.loads(execute(hub, "curl", "-fsS", "http://127.0.0.1:8080/api/info").stdout)
            assert info["api_version"] == 2
            closed = execute(hub, "curl", "--connect-timeout", "2", "-kfsS", "https://127.0.0.1:443", check=False)
            assert closed.returncode != 0, "Unclaimed Serve must remain closed"
            for identity in (tail, hub):
                inspect = json.loads(run(["docker", "inspect", identity]).stdout)[0]
                assert not inspect["HostConfig"].get("PortBindings"), "No public host ports"
            print("Pinned sidecar passed: NeedsLogin, TUN, legacy firewall capabilities, shared namespace, private unclaimed Hub")
        finally:
            if not helper:
                run(["docker", "compose", "-f", str(config), "down", "--volumes"], check=False)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
