#!/usr/bin/env python3
"""Copy only visual pet data from local third-party fixtures into a package stage."""

from __future__ import annotations

import argparse
import shutil
from pathlib import Path

EXACT_FILES = {
    "actions.xml",
    "behaviors.xml",
    "descript.txt",
    "pet.json",
    "surfaces.txt",
    "spritesheet.webp",
}
SAFE_SUFFIXES = {".png", ".qoi", ".wlshm"}


def is_visual_asset(path: Path) -> bool:
    return path.name.lower() in EXACT_FILES or path.suffix.lower() in SAFE_SUFFIXES


def collect(source: Path, destination: Path) -> int:
    copied = 0
    if not source.is_dir():
        return copied
    for path in source.rglob("*"):
        if not path.is_file() or not is_visual_asset(path):
            continue
        relative = path.relative_to(source)
        target = destination / "community" / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, target)
        copied += 1
    return copied


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    copied = collect(args.source, args.destination)
    print(f"collected {copied} visual pet files into {args.destination}")
    return 0 if copied else 2


if __name__ == "__main__":
    raise SystemExit(main())
