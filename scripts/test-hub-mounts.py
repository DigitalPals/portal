#!/usr/bin/env python3
"""Verify real same-filesystem bind mounts are refused in a private namespace."""
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import uuid

ROOT = Path(__file__).resolve().parents[1]


def main():
    helper = shutil.which("agent-resource")
    if helper and not os.environ.get("AGENT_RESOURCE_LEASE"):
        return subprocess.call([helper, "run", "--task", "Portal Hub bind-mount deletion guard tests", "--ttl", "10m", "--", sys.executable, __file__])
    if not helper and os.environ.get("GITHUB_ACTIONS") != "true":
        raise RuntimeError("Use agent-resource or a disposable GitHub Actions runner")
    if "--namespace" not in sys.argv:
        mapping = [] if os.geteuid() == 0 else ["--user", "--map-root-user"]
        return subprocess.call(["unshare", *mapping, "--mount", "--net", sys.executable, __file__, "--namespace"])
    spec = importlib.util.spec_from_file_location("deployment", ROOT / "deploy/hub/portal_hub_deploy.py")
    deployment = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(deployment)
    # A separate mount namespace and propagation boundary prevent host changes.
    subprocess.run(["mount", "--make-rprivate", "/"], check=True)
    with tempfile.TemporaryDirectory(prefix="portal-mount-test-", dir=os.environ.get("AGENT_RESOURCE_WORKSPACE")) as temp:
        base = Path(temp)
        manager = deployment.Manager({"protocol_version": 1, "action": "backup", "deployment_id": str(uuid.uuid4())}, root=base)
        state = manager.directory / "hub-data"
        state.mkdir(parents=True)
        external = base / "external data"
        external.mkdir()
        sentinel = external / "keep.txt"
        sentinel.write_text("external data must survive")
        for source, destination in [(external, state / "directory mount"), (sentinel, state / "file mount")]:
            if source.is_dir():
                destination.mkdir()
            else:
                destination.touch()
            subprocess.run(["mount", "--bind", str(source), str(destination)], check=True)
            try:
                assert source.stat().st_dev == destination.stat().st_dev
                try:
                    manager.create_snapshot(manager.directory / "unused.tar.gz")
                except deployment.DeployError as error:
                    assert error.code == "unsafe_path"
                else:
                    raise AssertionError("Backup crossed a real bind mount")
                assert sentinel.read_text() == "external data must survive"
                assert not (manager.directory / "unused.tar.partial").exists()
            finally:
                subprocess.run(["umount", str(destination)], check=True)
        manager.check_no_mounts(manager.directory)
    print("Mount guards passed: real same-filesystem directory/file binds, escaped paths, external data retained")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
