#!/usr/bin/env python3
"""Generate PNG icons from SVG source files at various sizes for GitHub."""

import cairosvg
import os
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent

# Icon sizes for various use cases
ICON_SIZES = [16, 32, 48, 64, 128, 256, 512]

# Profile picture sizes (GitHub recommends 500x500)
PROFILE_SIZES = [200, 460, 500]

def convert_svg_to_png(svg_path: Path, output_path: Path, size: int):
    """Convert SVG to PNG at specified size."""
    cairosvg.svg2png(
        url=str(svg_path),
        write_to=str(output_path),
        output_width=size,
        output_height=size,
    )
    print(f"  Created: {output_path.name} ({size}x{size})")

def main():
    output_dir = SCRIPT_DIR / "png"
    output_dir.mkdir(exist_ok=True)

    # Source SVG files
    darkmode_svg = SCRIPT_DIR / "logo-darkmode.svg"
    circle_svg = SCRIPT_DIR / "logo-darkmode-circle.svg"
    circle_transparent_svg = SCRIPT_DIR / "logo-darkmode-circle-transparent.svg"

    print("\n=== Generating Dark Mode Icon PNGs ===")
    print("Square icons (for favicons, app icons, etc.):")
    for size in ICON_SIZES:
        output_path = output_dir / f"logo-darkmode-{size}x{size}.png"
        convert_svg_to_png(darkmode_svg, output_path, size)

    print("\n=== Generating Profile Picture PNGs (with dark background) ===")
    print("Circular icons with dark background (for GitHub profile):")
    for size in PROFILE_SIZES:
        output_path = output_dir / f"logo-darkmode-circle-{size}x{size}.png"
        convert_svg_to_png(circle_svg, output_path, size)

    print("\n=== Generating Profile Picture PNGs (transparent) ===")
    print("Circular icons with transparent background:")
    for size in PROFILE_SIZES:
        output_path = output_dir / f"logo-darkmode-circle-transparent-{size}x{size}.png"
        convert_svg_to_png(circle_transparent_svg, output_path, size)

    print("\n=== Summary ===")
    print(f"Generated {len(list(output_dir.glob('*.png')))} PNG files in {output_dir}/")
    print("\nRecommended for GitHub:")
    print("  - Profile picture: logo-darkmode-circle-500x500.png")
    print("  - Organization avatar: logo-darkmode-circle-500x500.png")
    print("  - Favicon: logo-darkmode-32x32.png")
    print("  - Social preview: Create custom 1280x640 image with logo")

if __name__ == "__main__":
    main()
