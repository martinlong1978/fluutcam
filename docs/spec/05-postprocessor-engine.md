# 05 — Post-Processor Engine

## 5.1 Concept

Many CAM products use flat, declarative post-processor templates, one self-contained file per
machine. Most of their content is repeated from file to file, because G-code is largely the
same across controllers.

FluutCAM keeps the declarative approach and adds three things:

1. **Layering.** A post extends a parent and may add mixins. Broad definitions (RS-274 G-code,
   then a controller dialect) are refined by more specific ones (vendor, machine, user). §5.2.
2. **Vectric-style templates.** Blocks named by toolpath event, with lines containing
   `[PLACEHOLDER]` variables, and per-word formatting as in Vectric `VAR` definitions. The
   placeholder names follow Vectric's, so imported Vectric posts convert directly (§5.10.1).
3. **Typed segments.** The engine receives lines, arcs, helices, rotary sweeps and splines
   (03 §3.4), and declares per post which of these the controller accepts. §5.5.

Posts stay declarative. There are no loops and no general scripting. Conditional lines use a
small expression language (§5.4.3). This keeps posts safe to share and easy to diff.
Procedural posts from other products run only through the sandboxed host in §5.10.3.

## 5.2 Layering and resolution

### 5.2.1 Layer tree

```
fluut/gcode                       RS-274/NGC core: words, modal groups, G0/G1/G2/G3, G17-19
├─ fluut/fanuc                    Fanuc dialect: O-number, %, G43 H, M6 Tn, G28 home
│  ├─ fluut/haas                  Haas specifics
│  └─ …
├─ fluut/linuxcnc                 G64 blending, G93, O-codes off, T M6 G43
├─ fluut/grbl                     no tool change, limited words, $-settings, laser M4
│  └─ vendor/openbuilds-blackbox
├─ fluut/marlin
├─ fluut/mach3                    also Mach4, UCCNC
├─ fluut/masso
└─ …

mixins (feature fragments, combinable):
  fluut/mixin/rotary-a            A-axis words, rotary block, wrap rules
  fluut/mixin/inverse-time-feed   G93 / F as 1/min for rotary moves
  fluut/mixin/atc                 automatic tool change sequence
  fluut/mixin/no-arcs             linearize all arcs
  fluut/mixin/coolant-m7-m8
  fluut/mixin/laser
```

Users never edit built-in layers. They extend them. A built-in layer change ships as a new
version.

### 5.2.2 Merge rules

Resolution order (each later step wins):

```
resolve(post):
    base   = post.extends ? resolve(post.extends) : empty
    for m in post.mixins: base = merge(base, resolve(m))
    result = merge(base, post.own_layer)
    if posting for a machine: result = apply(result, machine.post_overrides)   // 02 §2.3
    validate(result)                                                          // §5.2.5
```

| Item kind | Merge |
|---|---|
| Scalar (`output.extension`, `words.X.decimals`, …) | child replaces |
| Table (`words.X`, `arcs`, `rotary`) | deep merge by key |
| Block (`blocks.header`, …) | `mode = "replace"` (default), `"append"`, `"prepend"`, `"remove"`, or `"patch"` |
| Block line with `id` (`mode = "patch"`) | child may `replace_line`, `remove_line`, or insert `before`/`after` that id |
| Other arrays | child replaces |

Line ids allow precise overrides without copying a whole block:

```toml
# parent (fluut/fanuc)
[blocks.header]
lines = [
  { id = "pct",   line = "%" },
  { id = "units", line = "G21" },
  { id = "safe",  line = "G17 G40 G49 G80 G90" },
]

# child
[blocks.header]
mode = "patch"
replace_line = { safe = "G17 G40 G49 G80 G90 G94" }
after        = { units = ["G54"] }
```

Rules:
- Missing parent or mixin: error. Never fall back silently.
- Cycle in `extends`: error. Maximum depth 16.
- Two mixins setting the same key: the later mixin wins, and the resolver reports the conflict
  as a warning.

### 5.2.3 Machine post overrides

A machine (02 §2.3) MAY override resolved post keys by path (`"words.F.decimals" = 0`). This keeps
small machine-specific differences out of the post tree.

### 5.2.4 Lessons from Vectric `POST_BASE`

Vectric supported post inheritance with `POST_BASE` and removed it after V11.0 because it
"caused problems" (Vectric migration guide; no detailed reason given). Failure modes such a
feature invites, and the rules that address them:

| Failure mode | Rule |
|---|---|
| Editing a base silently changes every child's output | `extends` names a major version (`@1`; versions are `major.minor`, only a major bump may change output). Built-in layers are immutable. Each posted program records the exact resolved layer versions (§5.9). When a base changes, dependent posts are flagged "changed since last verified". |
| Base file missing or renamed on another PC | Missing parent is an error (§5.2.2). Projects store the resolved post snapshot, so a project still posts identically on a machine without the library. |
| Nobody can tell where a line came from | `explain` view: for every block line and word setting, show the layer that supplied it. |
| Need to share one file | `flatten` export: writes a standalone post with no `extends`. |

### 5.2.5 Validation

The resolved post MUST be checked for: undefined placeholders, blocks required by enabled
capabilities (e.g. `arc_cw` when `arcs.enabled`), inconsistent limits (e.g. `centre_mode = "R"`
with `max_sweep_deg > 180`), and unknown keys. Errors block posting. Warnings are shown.

## 5.3 Words

Equivalent of Vectric `VAR NAME = [VO|WO|CS|VF|MX]`:

```toml
[words.X]
placeholder = "X"          # [X] in templates                    (Vectric VO)
when        = "changed"    # always | changed                     (Vectric WO)
prefix      = "X"          #                                      (Vectric CS)
decimals    = 3            #                                      (Vectric VF)
scale       = 1.0          #                                      (Vectric MX)
sign        = "negative"   # negative | always
trailing_zeros = false
leading_zero   = true      # "0.5" vs ".5"
pad_width   = 0
```

Global number settings: `output.decimal_char`, `output.line_ending`
(Vectric `LINE_ENDING`), `output.substitute` (Vectric `SUBSTITUTE`).

### 5.3.1 Modal state

The engine tracks modal groups (motion, plane, distance mode, feed mode, units, cutter
compensation, tool length offset). A word or G-code marked `modal = true` is emitted only when
its group changes. This generalizes Vectric's `FIRST_*` block pairs.

## 5.4 Blocks

### 5.4.1 Events

| Block | Fired when | Vectric equivalent |
|---|---|---|
| `header` | program start | `HEADER` |
| `footer` | program end | `FOOTER` |
| `tool_change` | tool differs from previous | `TOOLCHANGE` |
| `setup_start` | each Setup | — |
| `rotary_index` | indexed Setup start: move rotary axis to the index angle, after the turn retract (§5.6) | — |
| `rotary_clamp` / `rotary_unclamp` | after indexing / before rotary motion or re-index; lines default to machine `clamp`/`unclamp` codes | — |
| `setup_end` | each Setup | — |
| `toolpath_start` | each Toolpath | `NEW_SEGMENT` |
| `toolpath_end` | each Toolpath | — |
| `spindle_on` / `spindle_off` | spindle state change | `SPINDLE_ON` |
| `coolant_on` / `coolant_off` | coolant state change | — |
| `rapid` | rapid move | `RAPID_MOVE`, `INITIAL_RAPID_MOVE` |
| `feed` | linear cutting move | `FEED_MOVE`, `FIRST_FEED_MOVE` |
| `plunge` | linear move with downward tool-axis component | `PLUNGE_MOVE`, `FIRST_PLUNGE_MOVE` |
| `retract` | retract move | `RETRACT_MOVE` |
| `arc_cw` / `arc_ccw` | ArcSegment | `CW_ARC_MOVE`, `CCW_ARC_MOVE` (+ `FIRST_`) |
| `helix_cw` / `helix_ccw` | HelixSegment | `CW_HELICAL_ARC_MOVE`, … |
| `rotary` | RotarySegment | (wrapped `FEED_MOVE` with `[A]`) |
| `spline` | SplineSegment | — |
| `feed_change` | feed value changes | `FEED_RATE_CHANGE` |
| `dwell` | dwell | `DWELL_MOVE` |
| `cutter_comp_left` / `cutter_comp_right` / `cutter_comp_off` | start / end of a pass with control or wear compensation (G41/G42/G40) | `CUTTER_COMP_LEFT/RIGHT/OFF` |
| `cycle_start` | first hole of a drill toolpath: canned cycle with its parameters (G81/G82/G83/G73/G84/G85…) | — |
| `cycle_point` | each further hole position | — |
| `cycle_end` | after the last hole (G80) | — |
| `manual_nc` | `manual_nc` toolpath: stop, optional stop, comment, dwell, pass-through | — |
| `comment` | setup/toolpath/tool comments | — |

`first_*` variants (e.g. `first_feed`) MAY be defined. They fire for the first move of a type
after a different motion. Most posts do not need them, because modal tracking (§5.3.1) handles
the common case.

### 5.4.2 Placeholders

Placeholder names follow Vectric so imported posts keep working: `[X] [Y] [Z] [A] [B]`,
`[I] [J]` (incremental centre), `[IA] [JA]` (absolute centre), `[F] [FP] [FC]`, `[S]`, `[T]`,
`[TP]`, `[N]`, `[TOOLNAME]`, `[TDIA]`, `[SAFEZ]`, `[XH] [YH] [ZH]`, `[XMIN] … [ZMAX]`,
`[DATE] [TIME]`, `[TOOLPATH_NAME]`, `[FILE_NOTES]`, `[FI]` (inverse time), `[Radius]`.

Additions: `[K]`, `[KA]` (third arc centre word for XZ/YZ arcs), `[R]`, `[PLANE]` (G17/G18/G19),
`[MACHINE_NAME]`, `[SETUP_NAME]`, `[WORK_OFFSET]`, `[INDEX_A]`, `[INDEX_B]`, `[POST_ID]`, `[FLUUT_VERSION]`.

### 5.4.3 Conditional lines

```toml
[blocks.tool_change]
lines = [
  "M5",
  { when = "machine.tool_changer == 'atc'",    line = "T[T] M6" },
  { when = "machine.tool_changer == 'manual'", line = "M0 (Change to [TOOLNAME])" },
  "G43 H[T]",
]
```

**[DECIDED — OQ-14]** A small own expression language, parsed in Rust. No assignment, loops,
user functions, or access outside the context paths below.

```ebnf
expr     = or ;
or       = and , { "||" , and } ;
and      = not , { "&&" , not } ;
not      = [ "!" ] , cmp ;
cmp      = sum , [ ( "==" | "!=" | "<" | "<=" | ">" | ">=" ) , sum ] ;
sum      = term , { ( "+" | "-" ) , term } ;
term     = unary , { ( "*" | "/" ) , unary } ;
unary    = [ "-" ] , atom ;
atom     = number | string | "true" | "false" | path | call | "(" , expr , ")" ;
path     = ident , { "." , ident } ;           (* roots: tool machine setup toolpath state *)
call     = ( "abs" | "round" | "min" | "max" ) , "(" , expr , { "," , expr } , ")" ;
string   = "'" , { char - "'" } , "'" ;
```

Types: number (`f64`), string, boolean. Comparing different types is a validation error at post
load (§5.2.5), as is an unknown path. Evaluation cannot fail at run time.

## 5.5 Capabilities and limits

```toml
[arcs]
enabled        = true
planes         = ["XY", "XZ", "YZ"]        # or ["ANY"]; others are linearized
centre_mode    = "ijk_incremental"         # ijk_incremental | ijk_absolute | r
min_radius     = 0.05                      # Vectric MIN_ARC_RADIUS
max_radius     = 5000                      # Vectric MAX_ARC_RADIUS
max_sweep_deg  = 360                       # ≤ 180 when centre_mode = r
radius_tolerance = 0.002                   # controller's start/end radius check (§5.8)

[helix]
enabled = true
planes  = ["XY"]

[rotary]
enabled            = true
max_angle_per_line = 90                    # Vectric MAXIMUM_ROTATION
linear_with_rotary = true                  # X/Z interpolated in the same line

[spline]
enabled = false                            # not in v1 (OQ-07)

[cycles]                                   # canned drilling cycles
enabled = true                             # false: expand to rapid/feed/dwell moves
supported = ["drill", "spot", "peck", "chip_break", "tap", "ream", "bore"]

[output]
max_file_size = 0                          # 0 = no split; Vectric TAPE_SPLITTING
max_segment_length = 0                     # Vectric MAXIMUM_LENGTH
```

A segment type or geometry the post cannot accept is linearized to the segment's own machining
tolerance. Output therefore stays valid for every controller.

## 5.6 Emission algorithm

```
post = resolve(machine.post) with machine overrides           // §5.2
validate(post)                                                // §5.2.5
emit block header
for setup in setups:                                          // machining order (03 §3.9)
    emit block setup_start                                    // work offset, comments
    if setup.kind in (indexed, rotary) and setup.turn_z and the tool has moved:
        emit block rapid with only [Z] = setup.turn_z          // turn clearance (03 §3.9.5)
    if setup.kind == indexed:
        if rotary clamped: emit block rotary_unclamp
        emit block rotary_index                               // [INDEX_A] = resolved angle (03 §3.9.5)
        emit block rotary_clamp
    if setup.kind == rotary and rotary clamped: emit block rotary_unclamp
    for tp in setup.toolpaths:
        if tp.tool != prev_tool: emit block tool_change
        emit block toolpath_start
        for pass in tp.result.passes:
            for seg in pass.segments:
                if not post.accepts(seg): seg_list = linearize(seg, seg.tolerance)
                else:                     seg_list = split(seg, post limits)   // sweep, angle, length
                for s in seg_list:
                    event = classify(s)          // rapid | feed | plunge | retract | arc_* | helix_* | rotary | spline | cycle_*
                    if feed changed and post has feed_change: emit block feed_change
                    emit block event             // placeholders formatted per §5.3, §5.8
        emit block toolpath_end
    emit block setup_end
emit block footer
split file if output.max_file_size exceeded
```

Setups that share a machine are posted into one program, in machining order. Setups on
different machines produce separate programs. The user MAY also post a single Setup or a single
Toolpath.

Linearization and splitting happen in the post engine. The toolpath stays exact and
machine-independent, so switching posts never needs recalculation.

**Turn retract.** A turns at the start of an indexed Setup (`rotary_index`) and on the first move
of a rotary Setup. Before either, the engine fires `rapid` with only `[Z]` set to the Setup's turn
height (03 §3.9.5), in the new Setup's work offset, whose zero lies on the rotary axis. It does so
only once a motion line has been written: the program's first turn assumes the machine starts
clear, as its first rapid does. A post that wants a machine-coordinate retract as well can add it
to `rotary_index`.

## 5.7 Rotary feed

| Method | Output | Computation |
|---|---|---|
| `degrees` | F in deg/min | F = surface feed / r × (180/π) |
| `inverse_time` | `G93` + F = 1/min (`[FI]`) | F = surface feed / path length |
| `linear_only` | F as given | no conversion |

`r` is the CL radius stored on each RotarySegment (04 §4.7.5). Set by `feed.rotary_method`
(Vectric `INVERSE_TIME_MODE`).

## 5.8 Number formatting and arc consistency

The post engine is the only place toolpath coordinates are rounded (01 §1.2a).

1. Each coordinate word is rounded from the exact `f64` value of that segment endpoint. It is
   never derived from a previously rounded value, so rounding error does not accumulate.
2. Incremental distance mode (G91): deltas are computed between the *rounded* absolute positions,
   so the controller's running position equals the rounded absolute position at every line.
3. Arc centres: after rounding start S′ and end E′, the centre is moved to the point C′ on the
   perpendicular bisector of S′E′ nearest the exact centre C, then rounded. If
   `| |C′−S′| − |C′−E′| | > arcs.radius_tolerance` after rounding, the arc is emitted with one more
   decimal place if allowed, else linearized. This prevents controller errors of the kind
   "radius to end of arc differs from radius to start".
4. `R` mode: R is computed from S′, E′ and C′, not from the exact radius.

## 5.9 Provenance and safety

Every output file starts with a comment block (in the post's comment syntax) recording:
FluutCAM version, project name, resolved post id, each layer id and version, and a hash of
the resolved post.

Risk: verbatim command lines in user-editable posts
can inject arbitrary machine commands. Rules:

- Built-in layers are read-only and integrity-checked (hash) at load.
- When a user or imported layer has changed since the project was last posted, the user sees
  a diff of the resolved post before posting.
- Imported posts are marked `origin = "imported"` until the user has reviewed them.

## 5.10 Importing definitions from other products

Every import produces a native definition plus an **import report**: mapped keys, unsupported
features (by name), approximations made, and verification result.

### 5.10.1 Vectric `.pp`

Declarative. Full static conversion.

| Vectric | Native |
|---|---|
| `POST_NAME`, `FILE_EXTENSION`, `UNITS` | `name`, `output.extension`, `output.units` |
| `LINE_ENDING`, `SUBSTITUTE` | `output.line_ending`, `output.substitute` |
| `LINE_NUMBER_START/INCREMENT/MAXIMUM` | `line_numbering.*` |
| `VAR NAME = [VO\|WO\|CS\|VF\|MX]` | `words.<VO>` (`when`, `prefix`, format → `decimals`/`sign`/`pad_width`, `scale`) |
| `begin <BLOCK>` + quoted lines | `blocks.<event>` per §5.4.1 table; `+` comments dropped |
| `MIN_ARC_RADIUS`, `MAX_ARC_RADIUS` | `arcs.min_radius`, `arcs.max_radius` |
| `MAXIMUM_ROTATION`, `INVERSE_TIME_MODE` | `rotary.max_angle_per_line`, `feed.rotary_method` |
| `ROTARY_WRAP_X/Y` | rotary axis letter + `parallel_to` (FluutCAM machines true rotary, 04 §4.7) |
| `MAXIMUM_LENGTH`, `TAPE_SPLITTING` | `output.max_segment_length`, `output.max_file_size` |
| `POST_BASE` (legacy) | `extends`, if the base file is imported too |
| `DIRECT_OUTPUT`, `PRINT_DIRECT`, drill-bank blocks, `JET_TOOL_*` | reported unsupported **[OPEN]** |

**Rebasing.** After converting, the importer SHOULD find the closest built-in dialect and offer a
minimal layer that `extends` it and holds only the differences. The rebased post is accepted
only if it produces byte-identical output to the standalone conversion on the reference
toolpath suite (§5.11). Otherwise the standalone post is kept.

### 5.10.2 DeskProto `.ppr`, `.mch`, `.ctr`

Declarative INI. Full static conversion; field mapping is specified with the importer.
Rebasing as §5.10.1.

### 5.10.3 Autodesk `.cps` (Fusion, HSMWorks, Inventor CAM)

These posts are JavaScript programs written against Autodesk's post API (event callbacks such as
`onOpen`, `onSection`, `onLinear`, `onCircular`, `onClose`, and helpers such as `createFormat`,
`createModal`, `writeBlock`). They cannot be converted statically in general.

**[DECIDED — OQ-13]** Not in v1. v1 imports declarative posts only (Vectric, DeskProto).
Fusion `.cps` support follows in a later release, using this approach:

1. **Hosted execution.** Run the `.cps` in a sandboxed pure-Rust JavaScript engine
   (`boa_engine`, 01 §1.8) with an emulation of the subset of the post API used by milling
   posts. FluutCAM toolpaths are fed in as API events. Sandbox: no file, network or process
   access; CPU-time and memory limits. Unsupported API calls stop posting with a named error.
2. **Capture and derive (optional).** Run the hosted post on the reference toolpath suite and try
   to infer an equivalent native declarative post. Accept it only if outputs are byte-identical.
   Otherwise keep the post hosted.

Fidelity target and API subset are to be decided after surveying a sample of public `.cps` posts.

### 5.10.4 Other procedural posts

SheetCAM (`.scpost`, Lua) and FreeCAD (Python) posts need a Lua or Python runtime. DC-03 limits
these to pure-Rust interpreters. **[DECIDED — OQ-13]** Not planned.

### 5.10.5 Tool libraries

Vectric `.vtdb` (SQLite tool database) and Fusion tool-library JSON convert to native tool
libraries (02 §2.4). SQLite reading needs a pure-Rust reader or a recorded FFI exception
(OQ-08, 01 §1.8.3).
Field mapping: [D16](../design/D16-tool-library-import.md) (Fusion built; Vectric `.vtdb` blocked on the table layout, which is not public). SQLite is read by a purpose-built read-only reader, not `rusqlite`, so no FFI exception is recorded.

## 5.11 Reference toolpath suite and testing

A fixed set of reference toolpaths (lines, arcs in each plane, helices, rotary sweeps, tool
changes, feed changes, edge cases such as zero-length and full-circle arcs) is maintained with
the post engine. It is used to:

- produce golden outputs for every built-in post (regression tests);
- verify rebasing and capture-and-derive (§5.10);
- show a preview when a user edits a post.

## 5.12 Decisions

| Id | Raised by | Decision |
|---|---|---|
| OQ-07 | DC-01 | No spline (G5.2/G6.2) output in v1; SplineSegments are linearized. Added when target controllers support it. |
| OQ-13 | DC-04 | v1: Vectric and DeskProto import only. Fusion `.cps` hosted execution later. SheetCAM, FreeCAD not planned. |
| OQ-14 | DC-04 | Own small expression grammar (§5.4.3) |
