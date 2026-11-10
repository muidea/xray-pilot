#!/usr/bin/env python3
"""Regenerate committed application icons from SVG; requires ImageMagick."""
import os
from pathlib import Path
import shutil
import subprocess

root = Path(__file__).resolve().parent.parent
assets = root / "crates/xray-manager-app/assets"
converter = shutil.which("magick")
if not converter and os.name != "nt":
    converter = shutil.which("convert")
if not converter:
    raise SystemExit("Install ImageMagick to regenerate icons (not needed to build or run).")
base = [converter, "-background", "none", str(assets / "xray-pilot.svg")]
for arguments, name in [
    ([], "xray-pilot.png"),
    (["-resize", "32x32"], "xray-pilot-tray.png"),
    (["-define", "icon:auto-resize=256,128,64,48,40,32,24,20,16"], "xray-pilot.ico"),
]:
    subprocess.run(base + arguments + ["-strip", str(assets / name)], check=True)
