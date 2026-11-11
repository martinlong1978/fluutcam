# D10 — Post Engine and Importers (`fluut-post`, `fluut-post-import`)

Implements: [S 05], OQ-07, OQ-13, OQ-14, CH-11 scheme. Depends on: `fluut-path`, `fluut-model`
(definition resolver), `ryu`/own formatter.

## 1. Pipeline

```
DefRef ──► resolve layers (D06 §4.3 + §2 post merger) ──► ResolvedPost (typed)
       ──► validate + compile templates and expressions (§3) ──► CompiledPost
CompiledPost + machine overrides + Setups' ToolpathResults ──► Emitter (§4) ──► text (files)
```

`CompiledPost` is cached by resolved-post hash, so posting many times compiles once.

## 2. Post-specific merge

The generic resolver (D06 §4.3) hands `blocks.*` tables with a `mode` key to the post merger:

```
merge_block(parent_lines, child):
    match child.mode:
        replace (default): child.lines
        append:            parent_lines ++ child.lines
        prepend:           child.lines ++ parent_lines
        remove:            []
        patch:
            lines = parent_lines
            for (id, text) in child.replace_line: lines[id].line = text        // error if id missing
            for id in child.remove_line: drop lines[id]
            for (id, new) in child.before: insert new before lines[id]
            for (id, new) in child.after:  insert new after lines[id]
```

Line ids are unique within a block; duplicates are a validation error. Provenance per line is
recorded (layer that supplied or last changed it) for `explain` [S 05 §5.2.4].

`flatten` serialises the resolved tree as a standalone post (no `extends`, no mixins, `mode`
keys removed, line ids kept).

## 3. Compilation

### 3.1 Templates

Each block line is parsed into tokens:

```rust
enum Tok { Lit(String), Word(WordId), Placeholder(PhId) }   // "G1 [X][Y] F[F]" → Lit("G1 "), Word(X), Word(Y), Lit(" F"), Word(F)
```

- `[NAME]` resolves first to a word defined in `[words.*]` (formatted per §4.3), else to a
  built-in placeholder [S 05 §5.4.2] (formatted with its default format), else a validation
  error listing the line and layer.
- A line whose words are all suppressed (modal `when = "changed"`, unchanged) and whose literal
  text is only a motion code already in effect is dropped, so no empty `G1` lines appear.

### 3.2 Expressions (OQ-14)

The EBNF of [S 05 §5.4.3] is parsed by a hand-written recursive-descent parser into an AST,
then type-checked against the context schema:

```
tool.number: Number, tool.name: String, tool.diameter: Number, …
machine.tool_changer: String, machine.has_rotary: Bool, …
setup.kind: String, setup.index_a: Number, setup.work_offset: String, …
toolpath.type: String, toolpath.name: String, …
state.coolant: String, state.spindle_on: Bool, state.rotary_clamped: Bool,
state.first_tool: Bool, state.moved: Bool (a motion line has been written), …
```

Unknown paths, type mismatches and functions with wrong arity fail at compile time
[S 05 §5.4.3]; evaluation is total (no runtime errors; division by zero yields 0 and a compile
warning when a literal zero divisor is seen).

## 4. Emitter

### 4.1 State

```rust
struct EmitState {
    modal: ModalState,          // motion, plane, distance, feed mode, units, comp, length offset, work offset
    pos: AxisWords,             // last *emitted* (rounded) value per axis word
    feed: Option<f64>, spindle: Option<(f64, Dir)>, coolant: Coolant,
    tool: Option<u32>, rotary_clamped: bool,
    line_no: u64, bytes_in_file: u64, file_index: u32,
}
```

### 4.2 Event loop

As [S 05 §5.6]. Classification of a segment into an event:

| Segment | Event |
|---|---|
| Line, motion Rapid | `rapid` (`first_rapid` if defined and previous motion differs) |
| Line, motion Plunge / Ramp with downward axis component | `plunge` |
| Line, motion Retract | `retract` |
| Line otherwise | `feed` |
| Arc | `arc_cw` / `arc_ccw` by sweep sign seen from the plane normal (G17: +Z, G18: +Y, G19: +X) |
| Helix | `helix_cw` / `helix_ccw` |
| Rotary | `rotary` |
| Cycle | `cycle_start` for the first in a run with equal parameters, then `cycle_point`; `cycle_end` after |
| Marker | `manual_nc` |

Acceptance check before classification (`post.accepts(seg)`): arc plane in `arcs.planes`,
radius in `[min_radius, max_radius]`, `arcs.enabled`, helix allowed, rotary allowed, spline
(never in v1, OQ-07), cycle kind in `cycles.supported`. Rejected segments are linearized
(`fluut-path::linearize(seg, seg.tol)`) or expanded (cycles → rapid/feed/dwell moves).

Splitting: arcs above `max_sweep_deg` into equal parts; rotary moves above
`max_angle_per_line`; lines above `max_segment_length`.

### 4.3 Number formatting [S 05 §5.3, §5.8]

```
value_out = round_half_away(value_exact · scale · unit_factor, decimals)
text = sign (per `sign`) + integer part (leading_zero rule) + decimal_char + fraction
       (trailing zeros stripped unless trailing_zeros = true; "-0" printed as "0")
```

- Values always come from the exact `f64` segment endpoints (§5.8 rule 1); never from previous
  rounded values.
- Incremental mode: delta = rounded(current) − rounded(previous) [S 05 §5.8 rule 2].
- Formatting is done with integer arithmetic after scaling to avoid binary→decimal surprises
  (e.g. 0.0005 rounding): `v·10^dec` is rounded with a correctly rounded decimal conversion
  (`ryu` shortest representation, then decimal string rounding).

### 4.4 Arc centre correction [S 05 §5.8 rule 3]

```
S′, E′ = rounded start, end;  C = exact centre (in plane coords)
m = (S′ + E′)/2;  u = unit perpendicular to (E′ − S′)
C′ = m + ((C − m)·u) u                      // nearest point to C on the bisector
C″ = round(C′) (as I,J,K words, incremental or absolute)
err = | |C″−S′| − |C″−E′| |
if err > arcs.radius_tolerance:
    if decimals < 6: retry with decimals + 1 for this arc's words
    else: linearize this arc within its tolerance
R mode: R = |C′ − S′| rounded; sign negative for sweeps > 180° (only if max_sweep_deg allows)
```

### 4.5 Rotary feed [S 05 §5.7]

`degrees`: F = feed / r · (180/π) with r = segment radius (from `Rotary.radius`, or the CL radius
for X-varying moves, averaged over the move). `inverse_time`: F = feed / L where L is the true
tool-tip path length of the move (helical length for X-varying rotary moves); emits G93 modally
and G94 when leaving rotary moves.

### 4.6 Cutter compensation

`contour_2d` with compensation "in control" or "wear" (D09 §2.4) uses the events
`cutter_comp_left`, `cutter_comp_right`, `cutter_comp_off` [S 05 §5.4.1] (Vectric equivalents
`CUTTER_COMP_LEFT/RIGHT/OFF`), fired on the lead-in and lead-out of compensated passes (CH-20,
resolved).

### 4.7 Output

- Streamed to a buffered writer; split into numbered files when `output.max_file_size` is
  exceeded (split at a safe point: after a retract, never inside a pass).
- Provenance comment block first [S 05 §5.9].
- Each toolpath starts with a rapid to the start of its first segment (the result begins
  where the tool must be, at the toolpath's clearance height; D08 §8.2). Without it the first
  move would run from wherever the tool change or the previous toolpath left the tool. Words
  already at that value are suppressed as usual (M3).
- A line map `Vec<(line_range, setup, toolpath, pass, segment)>` is produced for the Post-tab
  preview: clicking a G-code line highlights the segment in the viewport [S 08 §8.3].

## 5. Canned cycles

| Cycle kind | Typical Fanuc-family code | Expansion when unsupported |
|---|---|---|
| drill | G81 | rapid to R, feed to Z, rapid to R |
| spot (dwell) | G82 P | + dwell |
| peck | G83 Q | repeated feed by Q, rapid out to R, rapid back to last depth − retract gap |
| chip_break | G73 Q | feed by Q, short retract |
| tap | G84 | feed to Z at pitch × rpm, reverse spindle, feed out (only if spindle reversal block exists, else error) |
| ream / bore | G85 / G86 | feed in, feed out (or spindle stop + rapid out) |

Codes live in the post's `cycle_start` / `cycle_point` blocks, not in the engine. The engine
supplies `[CYCLE_Z]`, `[CYCLE_R]`, `[CYCLE_Q]`, `[CYCLE_P]`, `[CYCLE_F]` placeholders.

## 6. Importers (`fluut-post-import`)

### 6.1 Vectric `.pp`

Line-oriented parser:

```
'+' at line start           comment (dropped; first comment block kept as a TOML comment)
KEY = value                 global setting
VAR NAME = [VO|WO|CS|VF|MX] word definition
begin BLOCK_NAME            starts a block; following "quoted" lines belong to it
"…"                         block line; [VAR] placeholders kept verbatim
```

Mapping per [S 05 §5.10.1]. Value format `VF` (`FORMAT_FLAGS FIELD_WIDTH DECIMAL_SEPARATOR
DECIMAL_PLACES`, e.g. `1.4`, `+.3`, `0.1`) maps to `sign`, `pad_width`, `decimal_char`,
`decimals`. Vectric `FIRST_*` blocks map to `first_*` events. Unsupported settings and blocks are
listed in the import report. See CH-21 for format-field details to verify.

**Status (2026-11-11): built.** `fluut_post_import::vectric::{import, import_with_base}` and
`fluut posts import <file.pp> -o <post.toml> [--id] [--force]`; plan
[2026-11-11-vectric-post-import](../plan/2026-11-11-vectric-post-import.md). The output is a
standalone `fluut.post/1` post with `origin = "imported"`, checked by resolving, loading and
compiling it before it is returned (`import.pp.internal` if that fails). Rebasing (§6.3) is not
built.

Reading: `+` and `|` lines are comments (the first block is kept as a TOML comment); `KEY = value`
(a later key replaces an earlier, `import.pp.setting_duplicate`); `VAR` (case-insensitive) with 4
or 5 fields; `begin NAME` (case-insensitive); quoted lines, text after the closing quote other
than a `+` comment is reported; any other line, such as a bare `%`, is reported
(`import.pp.unknown_line`). UTF-8 with or without BOM, else Latin-1 with `import.file.encoding`.
A `SCRIPT` section is refused (`import.pp.script`). Limits: 1 MiB, 50 000 lines, 2 000 lines per
block, 1 000 `VAR`s, 4 `POST_BASE` levels; base names must be plain file names and acyclic; the
caller supplies the bytes (the CLI reads beside the input). Child settings, words (by output
name) and blocks replace the base's.

Words: key = output name (`VO`) in FluutCAM spelling (names match case-insensitively, as
Vectric's own posts write both `[Radius]` and `[RADIUS]`); `WO` `A` → `always`, `C` → `changed`,
anything else → `always` with `import.pp.var_format`; `CS` → `prefix` (`[nn]` codes expanded);
`VF` per CH-21 with `trailing_zeros = true`, `leading_zero = true`; `MX` → `scale`. Words whose
output name the engine supplies no value for are not written; their uses are reported where the
blocks use them.

Blocks:

| Vectric | FluutCAM |
|---|---|
| `HEADER`, `FOOTER` | `header` (sees the first toolpath's values, S 05 §5.4.2), `footer` |
| `TOOLCHANGE`, `NEW_SEGMENT` | `tool_change`, `toolpath_start`, every line `when = "!state.first_tool"` (the header sets up the first toolpath) |
| `RAPID_MOVE`, `FEED_MOVE`, `PLUNGE_MOVE`, `RETRACT_MOVE`, `FIRST_*` of each | `rapid`, `feed`, `plunge`, `retract`, `first_*`; no `RAPID_MOVE` → the feed lines; no `FEED_MOVE` → `FIRST_FEED_MOVE`, else refused (`import.pp.block_missing`) |
| `INITIAL_RAPID_MOVE` | `first_rapid`: its lines with `when = "!state.moved"`, then the first-rapid (or rapid) lines with `when = "state.moved"` |
| `CW_ARC_MOVE`, `CCW_ARC_MOVE`, `FIRST_*` | `arc_cw`, `arc_ccw`, `first_*`; `arcs.planes = ["XY"]`; `[IA] [JA]` → `centre_mode = "ijk_absolute"`; `[Radius]` → `centre_mode = "r"`, `max_sweep_deg = 90`, word key `R` with `placeholder = "Radius"`; a block with no centre the engine supplies → arcs off (`import.pp.arcs_linearized`) |
| `CW/CCW_HELICAL_ARC_MOVE`, `FIRST_*` | `helix_*`; the `…_PLUNGE_MOVE` variants stand in when the plain ones are missing, else they are reported (`import.pp.block_fallback`) |
| `DWELL_MOVE`, `FEED_RATE_CHANGE`, `SPINDLE_ON`, `CUTTER_COMP_*` | `dwell`, `feed_change`, `spindle_on`, `cutter_comp_*` |
| `JET_TOOL_*`, anything else | left out, `import.pp.block_unsupported` |
| (wrapped posts) | `rotary` = the first-feed (or feed) lines |

Lines: `[nn]` → the character (`[` written as `[LBRACKET]`; CR and LF split the line; other
control characters dropped, `import.pp.control_char`); `[POST_NAME]` → the name (with the
`SUBSTITUTE` pairs applied); placeholders the engine supplies kept; others removed together with
the letters written directly before them, as the emitter does for an absent value
(`import.pp.placeholder`); a comment made only of such placeholders, or a line left empty, is
dropped (`import.pp.line_dropped`). `[XMIN]`…`[ZMAX]` are toolpath extents
(`import.pp.placeholder_approx`). Lines are trimmed (`import.pp.whitespace`). A wrapped post
(`ROTARY_WRAP_X/Y = [-]A|B`) gets a rotary word (format of the wrapped axis word, prefix the
letter, `scale = -1` for `-`) written after `[X]` or `[Y]` in motion blocks, `rotary.enabled`,
`feed.rotary_method = "linear_only"` (`import.pp.rotary_wrap`).

Output settings: `comment_start`/`comment_end` from the post's own comment lines (`( … )`, `;` or
`'`); a post with none that does not look like G-code is warned (`import.pp.comment_syntax`),
because the provenance block (S 05 §5.9) is written in that style. No `LINE_ENDING` → CR LF.
`cycles.enabled = false` (Vectric posts have no cycle blocks; cycles are expanded).

Refused (no file): `import.file.invalid` (NUL bytes; no `POST_NAME` and no blocks),
`import.limit.*`, `import.pp.script`, `import.pp.units` (missing or not MM/INCHES),
`import.pp.inverse_time`, `import.pp.spindle_range`, `import.pp.rotary_wrap` (both axes, or no
axis letter), `import.pp.post_base`, `import.pp.block_missing`, `import.pp.internal`. The CLI adds
`import.format.unsupported`, `import.post.bad_id`, `import.output.exists`,
`import.output.unwritable`, `import.file.unreadable`.

Corpus run (2026-11-11, the 547 `.pp` files of a local Vectric Aspire 10.5 install, which stay on
the maintainer's computer): 187 converted without a warning, 339 with warnings, 21 refused (8
`spindle_range`, 6 `inverse_time`, 3 `script`, 2 `rotary_wrap`, 2 `block_missing`); every
converted post posted the reference suite. Warnings by files: `placeholder` 250, `line_dropped`
156, `setting_ignored` 111, `whitespace` 106, `unknown_line` 72 (mostly a bare `%`),
`comment_syntax` 45, `block_fallback` 41, `rotary_wrap` 32, `placeholder_approx` 26,
`block_unsupported` 24, `var_missing` 7, `file.encoding` 7, others under 5.

### 6.2 DeskProto `.ppr`, `.mch`, `.ctr`

INI reader; `.ppr` → native post, `.mch` → machine definition, `.ctr` → tool. Field mapping is
specified with the importer (M7).

### 6.3 Rebasing [S 05 §5.10.1]

```
candidates = built-in dialect posts (and their common mixin combinations)
for each candidate B:
    diff = structural diff between imported resolved tree and B's resolved tree
    score = size of diff (number of changed leaves + lines)
best = min score
child = minimal layer: extends = best, own = diff (block patches use line ids where possible)
verify: post the reference suite with child and with the standalone import; byte-identical?
    yes → offer rebased child (default) ; no → keep standalone, report why
```

### 6.4 Fusion `.cps` (after v1, OQ-13)

Not built in v1. The design reserves crate module `fluut-post-import::cps` and the sandbox rules
in [S 05 §5.10.3].

## 7. Built-in library (v1)

Root `fluut/gcode`; dialects `fluut/fanuc`, `fluut/haas`, `fluut/linuxcnc`, `fluut/grbl`,
`fluut/marlin`, `fluut/mach3` (also Mach4/UCCNC), `fluut/masso`, `fluut/centroid`; mixins
`rotary-a`, `inverse-time-feed`, `atc`, `no-arcs`, `coolant-m7-m8`, `cutter-comp`. Machines:
generic 3-axis router, generic 4-axis router, generic VMC. Each built-in post has golden output
files for the reference suite.

## 8. Reference toolpath suite [S 05 §5.11]

Synthetic `ToolpathResult`s built in code (no geometry needed): line moves in all axes; arcs in
XY/XZ/YZ with sweeps 1°, 90°, 179°, 181°, 359°, 360°; tiny and huge radii; helices; rotary moves
with and without X; tool changes; feed and spindle changes; cycles of each kind; markers;
setup changes with index moves and clamps; values exercising rounding (…5 at the last decimal,
−0.0000x).

## 9. Tests

- Golden files for every built-in post × reference suite (text compare).
- Vectric import: corpus of public `.pp` files; each converted post must emit the same G-code as
  a hand-checked expectation for a subset, and must load without errors for all. Built
  (2026-11-11): unit tests per value-format form; hand-written fixtures posted on the reference
  suite, with golden files, whose tool positions must contain, in order, every endpoint of the
  built-in post of the same family (`fluut/grbl`, `fluut/linuxcnc`, `fluut/gcode`,
  `fluut/linuxcnc-4x`, with the fixture's arc and helix settings and cycles expanded), and the
  same tools and spindle speeds; a corpus test over `FLUUT_VECTRIC_PP_DIR` (counts and codes
  only); a pair test over `FLUUT_VECTRIC_PAIRS_DIR` (`<post>.pp` beside G-code Vectric wrote
  with it) comparing number formats per address letter and line endings. Both env tests are
  skipped when unset.
- Arc correction property test: random arcs, random decimals; emitted arcs satisfy the radius
  check or are linearized.
- Expression fuzzing: random expressions never crash the compiler; type errors are reported.

## 10. Challenges

- CH-20 Cutter-compensation events (resolved: added to spec).
- CH-21 Vectric value-format semantics (resolved: owner supplies Vectric sample outputs;
  reading decided and corpus-checked 2026-11-11, outputs still to compare).
