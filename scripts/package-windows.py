#!/usr/bin/env python3
"""Package built Windows x64 binaries, user docs and isolated validation scripts."""

import argparse
import hashlib
from pathlib import Path
import subprocess
import tomllib
import zipfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary-dir", type=Path, default=Path("target/release"))
    parser.add_argument("--output-dir", type=Path, default=Path("dist"))
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    if subprocess.check_output(
        ["git", "status", "--porcelain", "--untracked-files=normal"], cwd=root, text=True
    ).strip():
        parser.error("Commit source changes before packaging so VERSION.txt identifies the inputs")
    version = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    revision = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=root, text=True
    ).strip()
    files = [(args.binary_dir / name, name) for name in (
        "xray-manager.exe", "xray-supervisor.exe", "xrayctl.exe"
    )]
    files += [(root / name, name) for name in ("README.md", "LICENSE")]
    for directory, pattern in (("docs", "*.md"), ("examples", "*.json"), ("scripts", "windows-*.ps1")):
        files += [(path, path.relative_to(root).as_posix()) for path in sorted((root / directory).rglob(pattern))]
    for source, _ in files:
        if not source.is_file():
            parser.error(f"Missing package input: {source}")
    args.output_dir.mkdir(parents=True, exist_ok=True)
    package = args.output_dir / f"xray-pilot-{version}-windows-x64.zip"
    with zipfile.ZipFile(package, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for source, name in files:
            archive.write(source, f"xray-pilot/{name}")
        archive.writestr(
            "xray-pilot/VERSION.txt",
            f"version={version}\nsource_commit={revision}\nplatform=windows-x64\n",
        )
    with zipfile.ZipFile(package) as archive:
        if archive.testzip() is not None:
            raise RuntimeError("Package CRC verification failed")
        for source, name in files:
            if archive.read(f"xray-pilot/{name}") != source.read_bytes():
                raise RuntimeError(f"Package input differs: {source}")
    digest = hashlib.sha256(package.read_bytes()).hexdigest()
    (args.output_dir / "SHA256SUMS").write_text(f"{digest}  {package.name}\n", encoding="utf-8")
    print(f"{package} ({package.stat().st_size} bytes, {len(files) + 1} files)")
    print(f"SHA256: {digest}")
    print(f"Source: {revision}")


if __name__ == "__main__":
    main()
