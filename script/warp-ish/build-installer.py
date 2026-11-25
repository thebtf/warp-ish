"""Build a Windows setup from an accepted warp-ish ZIP."""

import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path, PurePosixPath, PureWindowsPath
import re
import shutil
import subprocess
import tempfile
import zipfile


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def payload_path(name):
    path = PurePosixPath(name)
    is_reserved = getattr(os.path, "isreserved", lambda part: PureWindowsPath(part).is_reserved())
    if (not path.parts or str(path) != name or path.is_absolute()
            or any(part == ".." or part.endswith((".", " "))
                   or is_reserved(part)
                   or re.search(r'[\\:<>|"*?\x00-\x1f]', part) for part in path.parts)):
        raise ValueError(f"Invalid ZIP payload path: {name!r}")
    return path


def stage_payload(archive, files, destination):
    expected = {}
    folded = set()
    for record in files:
        relative = payload_path(record["path"])
        name = f"app/{relative}"
        if name.casefold() in folded:
            raise ValueError(f"Duplicate payload path: {name}")
        folded.add(name.casefold())
        expected[name] = record
    for program in ("warp-ish.exe", "warp-ish-mcp.exe"):
        if f"app/{program}" not in expected:
            raise ValueError(f"Missing executable in manifest: {program}")

    with zipfile.ZipFile(archive) as source:
        entries = [entry for entry in source.infolist() if not entry.is_dir()]
        if len(entries) != len(expected) or {entry.filename for entry in entries} != set(expected):
            raise ValueError("ZIP payload differs from the package manifest")
        for entry in entries:
            record = expected[entry.filename]
            if entry.file_size != record["size"]:
                raise ValueError(f"ZIP entry size differs: {entry.filename}")
            path = destination / record["path"]
            path.parent.mkdir(parents=True, exist_ok=True)
            with source.open(entry) as input_file, path.open("xb") as output_file:
                shutil.copyfileobj(input_file, output_file)
            if sha256(path) != record["sha256"]:
                raise ValueError(f"ZIP entry hash differs: {entry.filename}")


def build(package, compiler, output):
    metadata = read_json(package / "package.json")
    manifest_path = package / "manifest.json"
    manifest = read_json(manifest_path)
    acceptance = read_json(package / "acceptance.json")
    version = metadata["version"]
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?", version):
        raise ValueError(f"Invalid package version: {version!r}")
    archive_name = f"warp-ish-{version}-windows-x64.zip"
    archive = package / archive_name
    if (metadata["archive"] != archive_name
            or sha256(archive) != metadata["archive_sha256"]
            or sha256(manifest_path) != metadata["manifest_sha256"]
            or any(manifest[key] != metadata[key] for key in ("version", "fork_sha", "upstream_sha"))):
        raise ValueError("Package provenance mismatch")
    if (acceptance.get("status") not in {"accepted_local", "verified_local_rehearsal"}
            or any(acceptance.get(key) != metadata[key] for key in ("archive_sha256", "fork_sha"))):
        raise ValueError("Acceptance record does not cover this package")
    if output.exists():
        raise FileExistsError(f"Installer output already exists: {output}")

    script = Path(__file__).with_name("installer.iss")
    common = subprocess.check_output(
        ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
        cwd=script.parent, text=True,
    )
    scratch = Path(common.strip()).resolve(strict=True).parent / ".agent/tmp"
    scratch.mkdir(parents=True, exist_ok=True)
    name = f"warp-ish-{version}-windows-x64-setup.exe"
    kernel = ctypes.windll.kernel32
    kernel.SetErrorMode(kernel.GetErrorMode() | 0x8003)
    with tempfile.TemporaryDirectory(prefix="warp-ish-setup-", dir=scratch) as temp:
        temp = Path(temp)
        payload = temp / "payload"
        stage_payload(archive, manifest["files"], payload)
        subprocess.run(
            [compiler, f"/DPayloadDir={payload}", f"/DAppVersion={version}",
             f"/DOutputDir={temp}", script],
            cwd=script.parent, creationflags=subprocess.CREATE_NO_WINDOW, check=True,
        )
        setup = temp / name
        receipt = {
            "version": version, "installer": name, "installer_sha256": sha256(setup),
            "zip": archive_name, "zip_sha256": metadata["archive_sha256"],
            "manifest_sha256": metadata["manifest_sha256"],
            "fork_sha": metadata["fork_sha"], "upstream_sha": metadata["upstream_sha"],
            "iscc_sha256": sha256(compiler),
            "installer_recipe_sha256": sha256(script),
        }
        output.mkdir(parents=True)
        shutil.move(setup, output / name)
        (output / "installer.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    return output / name


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package-dir", required=True, type=Path)
    parser.add_argument("--iscc", required=True, type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    args = parser.parse_args()
    print(build(args.package_dir.resolve(strict=True), args.iscc.resolve(strict=True),
                args.output_dir.resolve()))
