#!/usr/bin/env python3
from pathlib import Path
import shutil
import subprocess
import tempfile

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
ATLAS = ROOT / "assets" / "default-pets" / "ohm-raven" / "spritesheet.webp"
SOURCE = ROOT / "packaging" / "OHMPet-icon.png"
MAC_ICON = ROOT / "packaging" / "macos" / "OHMPet.icns"
WINDOWS_ICON = ROOT / "packaging" / "windows" / "OHMPet.ico"


def build_source() -> Image.Image:
    atlas = Image.open(ATLAS).convert("RGBA")
    frame = atlas.crop((0, 0, 192, 208))
    alpha_box = frame.getchannel("A").getbbox()
    if not alpha_box:
        raise RuntimeError("OHM Raven idle frame is empty")
    raven = frame.crop(alpha_box)
    canvas = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
    max_size = 820
    scale = min(max_size / raven.width, max_size / raven.height)
    size = (max(1, round(raven.width * scale)), max(1, round(raven.height * scale)))
    raven = raven.resize(size, Image.Resampling.NEAREST)
    canvas.alpha_composite(raven, ((1024 - size[0]) // 2, (1024 - size[1]) // 2))
    return canvas


def build_macos_icon(source: Image.Image) -> None:
    if shutil.which("iconutil") is None:
        print("iconutil unavailable; skipped macOS icns generation")
        return
    with tempfile.TemporaryDirectory() as temp:
        iconset = Path(temp) / "OHMPet.iconset"
        iconset.mkdir()
        for points in (16, 32, 128, 256, 512):
            source.resize((points, points), Image.Resampling.NEAREST).save(
                iconset / f"icon_{points}x{points}.png"
            )
            source.resize((points * 2, points * 2), Image.Resampling.NEAREST).save(
                iconset / f"icon_{points}x{points}@2x.png"
            )
        MAC_ICON.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(
            ["iconutil", "-c", "icns", str(iconset), "-o", str(MAC_ICON)],
            check=True,
        )


def main() -> None:
    source = build_source()
    SOURCE.parent.mkdir(parents=True, exist_ok=True)
    source.save(SOURCE)
    WINDOWS_ICON.parent.mkdir(parents=True, exist_ok=True)
    source.save(
        WINDOWS_ICON,
        format="ICO",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )
    build_macos_icon(source)
    print(SOURCE)
    print(WINDOWS_ICON)
    print(MAC_ICON)


if __name__ == "__main__":
    main()
