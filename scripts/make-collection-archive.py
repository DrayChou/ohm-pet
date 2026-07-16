#!/usr/bin/env python3
"""Add sanitized local external pet fixtures to an existing OHM Pet ZIP."""

from __future__ import annotations

import argparse
import importlib.util
import shutil
import tempfile
import zipfile
from pathlib import Path


def load_collector(script: Path):
    spec = importlib.util.spec_from_file_location("ohm_pet_collector", script)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {script}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("base_zip", type=Path)
    parser.add_argument("external_assets", type=Path)
    parser.add_argument("output_zip", type=Path)
    args = parser.parse_args()

    collector = load_collector(Path(__file__).with_name("collect-external-pets.py"))
    with tempfile.TemporaryDirectory(prefix="ohm-pet-collection-") as temp:
        stage = Path(temp) / "stage"
        with zipfile.ZipFile(args.base_zip) as archive:
            archive.extractall(stage)
        roots = [
            path
            for path in stage.iterdir()
            if path.is_dir() and path.name != "__MACOSX"
        ]
        package_root = roots[0] if len(roots) == 1 and not (stage / "pets").exists() else stage
        pets = package_root / "pets"
        pets.mkdir(parents=True, exist_ok=True)
        copied = collector.collect(args.external_assets, pets)
        if copied == 0:
            raise RuntimeError("no external visual pet assets were collected")
        included = [
            "OHM-1「欧姆鸦」 (pets/ohm-raven)",
            "茶兔 / 果殼 (pets/community/tea-rabbit)",
            "女仆酱 / MaidChan (pets/community/maidchan)",
            "KuroShimeji (pets/community/shimeji-ee)",
            "UkagakaW Visual Pet (pets/community/ukagakaw)",
        ]
        (package_root / "COLLECTED-PETS.txt").write_text(
            "OHM Pet Collection / 宠物合集\n\n" + "\n".join(f"- {item}" for item in included) + "\n",
            encoding="utf-8",
        )
        args.output_zip.parent.mkdir(parents=True, exist_ok=True)
        if args.output_zip.exists():
            args.output_zip.unlink()
        shutil.make_archive(
            str(args.output_zip.with_suffix("")), "zip", stage
        )
        print(f"created {args.output_zip} with {copied} collected visual files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
