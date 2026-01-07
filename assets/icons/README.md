# BranchBox Icons

Dark mode logo variants for GitHub profiles, favicons, and other uses.

## Files

### SVG Sources
- `logo-darkmode.svg` - Square logo with bright purple (#a78bfa) for dark backgrounds
- `logo-darkmode-circle.svg` - Circular version with dark background (#1a1a2e)
- `logo-darkmode-circle-transparent.svg` - Circular version with transparent background

### PNG Exports (`png/`)

**Square icons** (transparent background):
| Size | File | Use Case |
|------|------|----------|
| 16x16 | `logo-darkmode-16x16.png` | Favicon (small) |
| 32x32 | `logo-darkmode-32x32.png` | Favicon (standard) |
| 48x48 | `logo-darkmode-48x48.png` | Small icon |
| 64x64 | `logo-darkmode-64x64.png` | Medium icon |
| 128x128 | `logo-darkmode-128x128.png` | Large icon |
| 256x256 | `logo-darkmode-256x256.png` | App icon |
| 512x512 | `logo-darkmode-512x512.png` | High-res icon |

**Circular icons** (dark background #1a1a2e):
| Size | File | Use Case |
|------|------|----------|
| 200x200 | `logo-darkmode-circle-200x200.png` | Small avatar |
| 460x460 | `logo-darkmode-circle-460x460.png` | Standard avatar |
| 500x500 | `logo-darkmode-circle-500x500.png` | **GitHub profile (recommended)** |

**Circular icons** (transparent background):
| Size | File | Use Case |
|------|------|----------|
| 200x200 | `logo-darkmode-circle-transparent-200x200.png` | Where background is provided |
| 460x460 | `logo-darkmode-circle-transparent-460x460.png` | Where background is provided |
| 500x500 | `logo-darkmode-circle-transparent-500x500.png` | Where background is provided |

## GitHub Recommendations

- **Profile picture**: `logo-darkmode-circle-500x500.png` (GitHub displays avatars in circles)
- **Organization avatar**: `logo-darkmode-circle-500x500.png`
- **Favicon**: `logo-darkmode-32x32.png`

## Regenerating PNGs

```bash
cd assets/icons
python3 generate-pngs.py
```

Requires: `pip install cairosvg`

## Color Palette (Dark Mode)

- Purple (primary): `#a78bfa`
- Green dot: `#4ade80`
- Cyan dot: `#22d3ee`
- Dark background: `#1a1a2e`
