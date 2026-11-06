#!/usr/bin/env python3
"""Round-trip a real Docker-save archive through the authenticated importer.

The selected tag must already exist. No containers are started, no images are
removed, and only the identical selected tag is loaded back into the daemon.
"""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import uuid

ROOT = Path(__file__).resolve().parents[1]


def main():
    spec = importlib.util.spec_from_file_location("deployment", ROOT / "deploy/hub/portal_hub_deploy.py")
    deployment = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(deployment)
    selected = sys.argv[1] if len(sys.argv) > 1 else deployment.DEFAULT_IMAGE
    deployment.validate_image(selected)
    def content():
        image = json.loads(subprocess.check_output(["docker", "image", "inspect", selected], text=True))[0]
        # A containerd store identifies the original OCI index and the imported
        # Docker manifest differently. Application config and layer identities
        # must remain identical; the manager pins the post-import content ID.
        return {key: image[key] for key in ("Config", "RootFS", "Architecture", "Os")}
    original = content()
    with tempfile.TemporaryDirectory(prefix="portal-image-import-") as temp:
        directory = Path(temp)
        archive = directory / "hub.tar"
        subprocess.run(["docker", "save", "-o", str(archive), selected], check=True)
        # Replace OCI metadata with a deliberately incompatible alternate graph.
        # A real daemon import must still succeed using only the authenticated
        # Docker manifest/config/layers retained by the canonicalizer.
        alternate = directory / "alternate.tar"
        with tarfile.open(archive, "r:") as source, tarfile.open(alternate, "w:") as target:
            for member in source:
                if member.name.lstrip("./") in {"index.json", "oci-layout"}:
                    continue
                target.addfile(member, source.extractfile(member) if member.isfile() else None)
            for name, value in [("oci-layout", {"imageLayoutVersion":"1.0.0"}), ("index.json", {"schemaVersion":2,"manifests":[{"mediaType":"application/vnd.oci.image.manifest.v1+json","digest":"sha256:" + "0" * 64,"size":123,"annotations":{"io.containerd.image.name":"localhost/unrelated:0.0.1","org.opencontainers.image.ref.name":"0.0.1"}}]})]:
                data = json.dumps(value).encode()
                member = tarfile.TarInfo(name)
                member.size = len(data)
                target.addfile(member, io.BytesIO(data))
        for path in (archive, alternate):
            with path.open("rb") as stream:
                digest = hashlib.file_digest(stream, "sha256").hexdigest()
            manager = deployment.Manager({"protocol_version":1, "action":"install", "deployment_id":str(uuid.uuid4()), "image_archive":str(path), "image_archive_sha256":digest}, root=directory)
            manager.load_archive(str(path), selected)
            assert content() == original, "Canonical import changed application config or layers"
    print("Actual image imports passed: authenticated Docker-save and alternate OCI metadata preserve application config and layers")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
