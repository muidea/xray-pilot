#!/usr/bin/env python3
"""Package built Windows x64 binaries, user docs and isolated validation scripts."""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import re
from pathlib import Path
import subprocess
import tomllib
import zipfile


def package_readme(text):
    """Flatten links to internal development docs that are not packaged."""
    def replace(match):
        label, target = match.group(1), match.group(2)
        if target.startswith("docs/validation-guide.md"):
            return f"[{label}](docs/验证说明.md)"
        if target.startswith("docs/"):
            return label
        return match.group(0)
    return re.sub(r"\[([^\]]+)\]\((docs/[^)]+)\)", replace, text)


def source_manifest(root):
    paths = (
        subprocess.check_output(
            ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
            cwd=root,
        )
        .decode("utf-8")
        .split("\0")
    )
    files = []
    for name in sorted(set(filter(None, paths))):
        path = root / name
        data = path.read_bytes() if path.is_file() else None
        files.append(
            {
                "path": name,
                "sha256": hashlib.sha256(data).hexdigest() if data is not None else None,
                "size": len(data) if data is not None else None,
            }
        )
    digest = hashlib.sha256(
        json.dumps(
            files, ensure_ascii=False, sort_keys=True, separators=(",", ":")
        ).encode("utf-8")
    ).hexdigest()
    return files, digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary-dir", type=Path, default=Path("target/release"))
    parser.add_argument("--output-dir", type=Path, default=Path("dist"))
    parser.add_argument(
        "--validation", action="store_true",
        help="Include validation instructions and source fingerprints; allow uncommitted source",
    )
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    dirty = bool(
        subprocess.check_output(
            ["git", "status", "--porcelain", "--untracked-files=normal"],
            cwd=root,
            text=True,
        ).strip()
    )
    if dirty and not args.validation:
        parser.error(
            "Commit source changes before packaging so VERSION.txt identifies the inputs"
        )
    version = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    revision = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=root, text=True
    ).strip()
    files = [
        (args.binary_dir / name, name)
        for name in ("xray-manager.exe", "xray-supervisor.exe", "xrayctl.exe")
    ]
    files += [(root / "LICENSE", "LICENSE")]
    readme_data = package_readme((root / "README.md").read_text(encoding="utf-8")).encode("utf-8")
    # Validation packages intentionally ship a curated docs subset. Design,
    # review, handoff and historical closeout notes stay in the source tree.
    files += [
        (root / "docs/validation-guide.md", "docs/验证说明.md"),
        (root / f"docs/releases/v{version}.md", "docs/发布说明.md"),
    ]
    files += [
        (path, path.relative_to(root).as_posix())
        for path in sorted((root / "examples").rglob("*.json"))
    ]
    # Keep user-facing dependency setup and the read-only proxy snapshot helper.
    # Smoke, portable-path, IPC and installer self-tests are developer audits.
    files += [
        (root / "scripts/windows-dependencies.ps1", "scripts/windows-dependencies.ps1"),
        (root / "scripts/windows-proxy-snapshot.ps1", "scripts/windows-proxy-snapshot.ps1"),
    ]
    for source, _ in files:
        if not source.is_file():
            parser.error(f"Missing package input: {source}")
    args.output_dir.mkdir(parents=True, exist_ok=True)
    now = datetime.now(timezone.utc)
    package = args.output_dir / f"xray-pilot-{version}-windows-x64.zip"
    if package.exists():
        parser.error(
            f"Package already exists: {package}. "
            "Use a new version for changed contents or a separate output directory."
        )
    metadata = f"version={version}\nsource_commit={revision}\nplatform=windows-x64\n"
    extra = {"README.md": readme_data}
    if args.validation:
        source_files, tree_hash = source_manifest(root)
        metadata += (
            f"package_kind=local-validation\nsource_dirty={str(dirty).lower()}\n"
            f"source_tree_sha256={tree_hash}\npackaged_at_utc={now.isoformat()}\n"
        )
        manifest = {
            "source_base_commit": revision,
            "source_dirty": dirty,
            "source_tree_sha256": tree_hash,
            "source_files": source_files,
            "package_inputs": [
                {
                    "path": name,
                    "sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
                    "size": source.stat().st_size,
                }
                for source, name in files
            ]
            + [
                {
                    "path": "README.md",
                    "sha256": hashlib.sha256(readme_data).hexdigest(),
                    "size": len(readme_data),
                }
            ],
        }
        extra["SOURCE-MANIFEST.json"] = json.dumps(
            manifest, ensure_ascii=False, indent=2
        ).encode("utf-8")
    extra["VERSION.txt"] = metadata.encode("utf-8")
    with zipfile.ZipFile(package, "x", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for source, name in files:
            archive.write(source, f"xray-pilot/{name}")
        for name, data in extra.items():
            archive.writestr(f"xray-pilot/{name}", data)
    with zipfile.ZipFile(package) as archive:
        if archive.testzip() is not None:
            raise RuntimeError("Package CRC verification failed")
        for source, name in files:
            if archive.read(f"xray-pilot/{name}") != source.read_bytes():
                raise RuntimeError(f"Package input differs: {source}")
        for name, data in extra.items():
            if archive.read(f"xray-pilot/{name}") != data:
                raise RuntimeError(f"Package metadata differs: {name}")
    if args.validation and source_manifest(root)[1] != tree_hash:
        package.unlink()
        raise RuntimeError("Source changed while packaging; rebuild and try again")
    digest = hashlib.sha256(package.read_bytes()).hexdigest()
    sums = "".join(
        f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n"
        for path in sorted(args.output_dir.glob("*.zip"))
    )
    (args.output_dir / "SHA256SUMS").write_text(sums, encoding="utf-8")
    print(f"{package} ({package.stat().st_size} bytes, {len(files) + len(extra)} files)")
    print(f"SHA256: {digest}")
    print(f"Source: {revision}")
    if args.validation:
        print(f"Local validation snapshot: {tree_hash} (dirty={dirty})")


if __name__ == "__main__":
    main()
