# Ribbon icons

The SVG files here are the masters of FluutCAM's icon set (spec 08 §8.9): one per ribbon command,
toolpath type and Setup kind. `build.rs` rasterises them with `resvg` at 16, 24, 32 and 48 px
(16 and 24 at 1x, 32 and 48 at 2x), into `OUT_DIR` only. Generated rasters are never committed,
and nothing is fetched at build time.

## Style rules

A test (`icons::tests::masters_parse_and_keep_the_style`) checks the marked rules.

- 24 px grid: `width="24" height="24" viewBox="0 0 24 24"`. *(checked)* Keep the drawing within
  2..22.
- Line style: on the root element `fill="none"`, `stroke="#16181D"`, `stroke-width="1.75"`,
  `stroke-linecap="round"`, `stroke-linejoin="round"`. No other stroke width. *(checked)*
- Two colours only: ink `#16181D` and the accent, cutter orange `#FF5B14`. *(checked)*
- No fills, except at most two small accent marks, such as a tool position dot or the quadrants
  of the work-zero symbol. *(checked)*
- The accent marks what the icon does (the tool, the direction, the part of the pattern that
  matters), and is a small part of the icon. Every icon has ink.
- Draw what the toolpath does seen from where the pattern reads best (top, side or end), and
  keep it legible at 16 px: at most four or five strokes of detail.

Do not rely on the colours beyond that. `build.rs` splits each raster into an ink mask and an
accent mask by colour, and the app tints them by theme: ink takes the theme's text colour, the
accent stays orange, and a disabled icon is grey throughout.

## Adding an icon

1. Copy a similar master and draw on the 24 px grid; keep the root element as it is.
2. Name it in kebab-case: `tp-<type>` for a toolpath type, otherwise the command. A name must not
   start with a family of Fluent ids (`setup-`, `sim-`, `ribbon-`, ...): the test that checks
   message ids reads string literals in the sources.
3. Return the name from `Command::icon` in `src/vm/ribbon.rs`. A test fails on a master that no
   command uses, and on a command whose icon is missing.

## Licence

All these icons were drawn for FluutCAM; none is copied or adapted from another icon set. They
are licensed like the code, under either the [Apache License, Version 2.0](../../../../LICENSE-APACHE)
or the [MIT licence](../../../../LICENSE-MIT), at your option.
