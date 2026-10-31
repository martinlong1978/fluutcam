# FluutCAM brand assets

The SVG files are the masters. Raster icons (ICO, ICNS, PNG) are generated from them when the
app and installers are packaged (D14 §5.2). Generated files are not committed.

`fluut-round3-sheet.png` shows every asset together.

## Files

| File | Use |
|---|---|
| `fluutcam-D-logo.svg` | Full logo (icon, "fluut" wordmark, "CAM") for light backgrounds. Used in the repository README. |
| `fluutcam-D-logo-dark.svg` | The same for dark backgrounds, with a transparent background. Used in the README for dark mode. |
| `fluut-D-logo.svg` | Icon and "fluut" wordmark, for light backgrounds. |
| `fluut-D-logo-dark.svg` | The same on a solid dark (`#16181D`) background. |
| `fluut-D-wordmark.svg` | The "fluut" wordmark alone. |
| `fluut-D-icon-stock.svg` | App icon: the cutter going into the stock. |
| `fluut-D-icon-solo.svg` | App icon: the cutter alone. |
| `fluut-round3-sheet.png` | Overview sheet of all of the above, at several sizes. |

## Colours

| Colour | Hex | Where |
|---|---|---|
| Ink | `#16181D` | icon tile, wordmark on light backgrounds, flute stripes on dark |
| Cutter orange | `#FF5B14` | the cutter and the "l" of the wordmark |
| Stock grey | `#8A93A0` | stock in the icon, "CAM" |
| Paper | `#F4F2EE` | wordmark on dark backgrounds |
| Flute white | `#FAF9F6` | flute stripes on light backgrounds |

## Dark variants

A dark variant changes only the wordmark:
- the ink wordmark strokes become paper (`#16181D` → `#F4F2EE`);
- the flute stripes in the "l" become ink (`#FAF9F6` → `#16181D`).

The icon and "CAM" stay as they are. `fluutcam-D-logo-dark.svg` was derived from
`fluutcam-D-logo.svg` by this rule.

## Licence

The assets are licensed like the code, under either the
[Apache License, Version 2.0](../LICENSE-APACHE) or the [MIT licence](../LICENSE-MIT), at your
option.
