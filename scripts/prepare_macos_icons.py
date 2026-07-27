#!/usr/bin/env python3
"""Normalize generated icon art and apply the macOS rounded-square alpha silhouette."""

from pathlib import Path
from io import BytesIO
import struct
import sys

from PIL import Image, ImageDraw, ImageFilter, ImageOps


ROOT = Path(__file__).resolve().parents[1]
ICON_DIR = ROOT / "assets" / "icons" / "macos"
VARIANTS = ("classic", "party", "contour")
SIZE = 1024
ICNS_TYPES = (
    (b"icp4", 16),
    (b"icp5", 32),
    (b"icp6", 64),
    (b"ic07", 128),
    (b"ic08", 256),
    (b"ic09", 512),
    (b"ic10", 1024),
)


def prepare(variant: str) -> None:
    source = ICON_DIR / f"{variant}-source.png"
    output = ICON_DIR / f"{variant}.png"
    if not source.exists():
        raise FileNotFoundError(source)

    with Image.open(source) as opened:
        artwork = ImageOps.fit(
            opened.convert("RGB"),
            (SIZE, SIZE),
            method=Image.Resampling.LANCZOS,
            centering=(0.5, 0.5),
        ).convert("RGBA")

    mask = Image.new("L", (SIZE, SIZE), 0)
    draw = ImageDraw.Draw(mask)
    draw.rounded_rectangle((1, 1, SIZE - 2, SIZE - 2), radius=216, fill=255)
    mask = mask.filter(ImageFilter.GaussianBlur(0.65))
    artwork.putalpha(mask)
    artwork.save(output, optimize=True)

    records = []
    for icon_type, size in ICNS_TYPES:
        resized = artwork.resize((size, size), Image.Resampling.LANCZOS)
        encoded = BytesIO()
        resized.save(encoded, format="PNG", optimize=True)
        payload = encoded.getvalue()
        records.append(icon_type + struct.pack(">I", len(payload) + 8) + payload)
    body = b"".join(records)
    icns_name = f"Ridgeline-{variant.title()}.icns"
    icns_path = ICON_DIR / icns_name
    icns_path.write_bytes(b"icns" + struct.pack(">I", len(body) + 8) + body)
    print(f"{output.relative_to(ROOT)} -> {icns_path.relative_to(ROOT)}")


def main() -> int:
    requested = tuple(sys.argv[1:]) or VARIANTS
    unknown = set(requested) - set(VARIANTS)
    if unknown:
        print(f"Unknown icon variant(s): {', '.join(sorted(unknown))}", file=sys.stderr)
        return 2
    for variant in requested:
        prepare(variant)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
