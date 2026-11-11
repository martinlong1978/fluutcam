# D16 — Tool Library Import (`fluut-post-import`)

Implements: [S 02 §2.5], [S 05 §5.10.5], [S 01 §1.7, §1.8.2, §1.8.3]. Builds on D06 §4 (tool
libraries), D07 §1 (tool shapes), D10 §6 (importers) and the tool model in
`crates/fluut-model/src/tools.rs`. Closes the **[OPEN]** field mapping of S 05 §5.10.5 for
Fusion. Vectric is designed to the point the public information allows (§7).

D10 §6 names the crate `fluut-post-import` for post and machine importers. Tool libraries live
there too: the spec lists "Vectric, DeskProto, Fusion importers" in one crate (S 01 §1.8.2), and
a second import crate would only repeat the untrusted-input rules.

## 1. Goals and non-goals

Goals:

1. Convert a Fusion tool-library JSON file to a native `fluut.tools/1` library, so that a user
   with a shop library can calculate with it at once.
2. Do it statically. The importer parses data and never runs it (S 01 §1.7).
3. Say what was changed. Every value that was converted, approximated or dropped is reported
   with a stable diagnostic code. Nothing is guessed silently.
4. Never produce a tool that the native parser rejects, or one that makes a toolpath misbehave.
   A tool the importer cannot map safely is left out, with a warning, not imported wrongly.

Non-goals: writing Fusion or Vectric files; syncing; importing machines, holders as a library of
their own, fixtures, or Fusion `.cps` posts (S 05 §5.10.3); a tool editor in the window (the
Library commands are not built, see Known limits).

## 2. Sources and what is known about them

### 2.1 Fusion tool-library JSON (built)

Fusion exports a tool library as a JSON file: an object with `data` (a list of tool entries)
and `version` (a number). Entries are objects with a `type` string, a `description`, a `unit`
string, a `geometry` object, a `post-process` object, a `start-values` object with a `presets`
list, and optionally a `holder` object. The geometry keys are short codes (`DC`, `OAL`, ...).

**What is publicly documented.** Autodesk publishes the post-processor tool object (the `tool`
variable of a `.cps` post: `diameter`, `cornerRadius`, `taperAngle`, `tipAngle`, `fluteLength`,
`shaftDiameter`, `overallLength`/`bodyLength`, `numberOfFlutes`, `threadPitch`, `number`,
`unit`, `type`) in its public post-processor documentation, and the Fusion manufacturing API
documentation describes the tool and tool-library objects. The JSON file itself has **no
public schema** that the author could fetch while writing this design (the Autodesk forum
thread on the file's variables was not retrievable). The key names below come from the
generally visible structure of exported files as discussed in public community threads, and
are the abbreviations of the documented tool properties. They are therefore **unverified
against an official schema**. The design takes three precautions:

- The importer reads only the keys in §3 and ignores the rest, counted in one summary warning.
- It checks every value (type, finite, range) and drops or skips on any doubt.
- The test fixtures are hand-written from this table, not copied from any file. A real file
  that disagrees with the table is a bug report, and the table and fixtures change together.

Each key listed in §3 is marked **sure** (named in the public `tool` object docs and stable
across the versions seen in public discussions) or **unsure** (kept only because the meaning is
clear from the abbreviation; check with a real file before relying on it).

### 2.2 Vectric `.vtdb` (designed, not built)

See §7. The file container is SQLite, which is public. The table and column layout is not
publicly documented, and Vectric files are proprietary samples that must not be copied into
the repository. The mapping is blocked until a user supplies a description of their own file
(§7.3).

## 3. Fusion field mapping

All source lengths are in the tool's `unit` and are converted to mm (§3.2). Angles are degrees.
A key that is absent leaves the native field unset, unless a default is listed.

### 3.1 Tool

| Source key | Native field | Notes | Confidence |
|---|---|---|---|
| `type` | `shape` | table in §4 | sure |
| `description` | `name` | control characters removed, 200 characters at most; also the base of `id` (§5) | sure |
| `unit` | (conversion factor) | §3.2; absent: millimeters, with warning `import.tool.bad_value` | sure |
| `geometry.DC` | `diameter` | must be above 0 and at most 10 000 mm, else the tool is skipped | sure |
| `geometry.RE` | `corner_radius` | bull and taper tools; for flat/ball/V/drill FluutCAM derives it | sure |
| `geometry.LCF` | `flute_length` | above 0 | unsure |
| `geometry.OAL` | `overall_length` | above 0; see §6 | sure |
| `geometry.SFDM` | `shank_diameter` | above 0 | sure |
| `geometry.TA` | `half_angle_deg` | V and taper tools; must be in (0, 90) | sure |
| `geometry.SIG` | `point_angle_deg` | drills; for a countersink, half of it is `half_angle_deg` | sure |
| `geometry.TP` | `pitch` | taps only | sure |
| `geometry.tip-diameter` | `tip_diameter` | V and taper tools; must be below `DC` | unsure |
| `post-process.number` | `number` | integer 0 to 99 999, else 0 with a warning | sure |
| `holder.segments[]` | `holder` | §6 | unsure |
| `start-values.presets[]` | `[[tool.preset]]` | §3.3 | sure |

Dropped (counted by key in `import.fields.dropped`, never run, never copied into the library):
`geometry.NOF` (FluutCAM has no flute count), `geometry.LB`, `geometry.CSP` and every other
geometry key; `post-process.*` except `number` (offsets, break control, turret, live); `vendor`,
`product-id`, `product-link`, `guid`, `BMC`, `GRADE`, `expressions` (text expressions are
**never** evaluated); `tool-coolant`. `centre_cutting` stays at its default (true): the meaning
of `CSP` is not confirmed.

### 3.2 Units

| Source `unit` | Factor to mm |
|---|---|
| `millimeters` | 1 |
| `centimeters` | 10 |
| `meters` | 1000 |
| `inches` | 25.4 |
| `feet` | 304.8 |

Any other value skips the tool (`import.tool.bad_value`). Feeds are length per minute in the
same unit and are converted with the same factor. Spindle speed is rev/min, unchanged.
The holder takes its own `unit` when it has one and otherwise the tool's.
The converted values are written with at most six decimals (a micrometre-level rounding: 6.35
stays 6.35).

### 3.3 Presets (feeds and speeds)

Fusion keeps cutting data in `start-values.presets`. FluutCAM keys cutting data by a material
name that a Setup selects (S 02 §2.4). Fusion presets are named by the user and their
`material` object is a query, not a name. So:

| Source key | Native field | Notes |
|---|---|---|
| `name` | `material` | the preset's name becomes the material name; empty becomes `default`; a second preset with the same name is dropped (`import.preset.dropped`) |
| `n` | `rpm` | at most 1 000 000 |
| `v_f` | `feed` | cutting feed, converted by §3.2 |
| `v_f_plunge` | `plunge` | converted |
| `stepover` | `stepover` | length; a Fusion preset's stepover is a length, converted |
| `stepdown` | `step_down` | converted |

Dropped: `f_z`, `f_n`, `v_c`, `n_ramp`, `v_f_ramp`, `v_f_leadIn`, `v_f_leadOut`, `v_f_retract`,
`v_f_transition`, `use-*` flags, `material` queries and coolant. FluutCAM does **not** derive a
feed from `f_z x flutes x n` (that would need `NOF` and a stable meaning of the `use-*` flags);
a preset without `v_f` is imported without `feed` and the toolpath asks for one
(`param.required`). At most 200 presets are read per tool.

The user must match the preset names to the Setup's `material` (the Setup's material list is
drawn from the loaded libraries, S 02 §2.4); the wiki page says so.

## 4. Tool type mapping

Fusion `type` strings are compared in lower case.

| Fusion type | Native shape | Notes |
|---|---|---|
| `flat end mill` | `flat` | |
| `ball end mill` | `ball` | |
| `bull nose end mill`, `radius mill` | `bull` | `RE` above 0; without it the tool becomes `flat` with `import.tool.approximated` |
| `chamfer mill` | `v` | `TA` is the half angle; `tip-diameter` the flat tip; without `TA` the tool is skipped |
| `counter sink` | `v` | `SIG` is the included angle, so `half_angle_deg = SIG / 2` |
| `tapered mill` | `taper` | `TA` half angle; always `import.tool.approximated` because the meaning of `DC` (tip or top) is not confirmed: check the size |
| `drill`, `center drill`, `spot drill` | `drill` | `SIG` point angle; 118 when absent (native default) |
| `tap right hand`, `tap left hand` | `drill` | `pitch` from `TP`; FluutCAM has no tapping cycle in the tool model, the `pitch` is stored for the drill toolpath |
| `face mill`, `slot mill` | `flat` | `import.tool.approximated` |
| anything else: `dovetail mill`, `lollipop mill`, `thread mill`, `reamer`, `counter bore`, `boring head`, `form mill`, `holder`, `probe`, laser, waterjet, plasma, turning and inserts | none | skipped with `import.tool.unsupported_type`; no guess |

Why skip rather than map to the closest shape: a lollipop or dovetail cutter mapped to `ball`
or `v` would give toolpaths that gouge. A missing tool is visible; a wrong tool is not.

## 5. Numbering, ids, duplicates and merging

**Library id and name.** `--id` gives the library id. Without it the id is
`fusion/<slug of the file stem>`. It must be lower-case ASCII letters, digits, `-`, `_` and `/`
(at most 100 characters), so that `library#tool` references parse. `--name` is the display
name, by default the file stem.

**Tool id.** `slug(description)` (lower case, runs of other characters become `-`, 40
characters); if the description is empty, `<shape>-<diameter>`. A tool with a number above 0
is prefixed `t<number>-`, so the id says which pocket of the changer it is, for example
`t1-6-mm-flat`. A repeated id gets `-2`, `-3`, ... in file order (`import.tool.id_duplicate`),
so an id depends only on the file's order and is stable between runs of the same file.

**Numbers.** The Fusion number becomes `number`. The same number on two tools is kept on both,
with `import.tool.number_duplicate` (Fusion allows it, for example for several presets of one
holder; a post that needs unique numbers should be told by the user). No renumbering: a tool
number is a machine fact.

**Merging into an existing library.** The importer always writes a **new** file and refuses to
overwrite one unless `--force` is given. It never edits a library in place. To combine an
import with your own tools, use the library mechanisms that exist: a library may `extends`
another (S 02 §2.4, D06 §4), and an extending library's tools replace the parent's with the
same id. Two imports are kept apart by their library ids. A tool-by-tool merge UI belongs to
the Library commands (not built).

## 6. Overall length and holders

FluutCAM's `overall_length` is the length from the tip to the spindle gauge line's reference
(the holder starts at the overall length, D11 §5); `Tool::gauge_length` adds the holder's length
to it. Fusion's `OAL` is the tool's overall length, tip to the end of the shank, which is the
same quantity. The importer takes `OAL` as it is. It does **not** derive an overall length from
the body length `LB`, the flute length or the holder (those are different quantities, and a
wrong length defeats the machine's travel check).

When `OAL` is absent or unusable, the tool is imported **without** `overall_length` and the
import warns `import.tool.length_missing`. FluutCAM itself then warns `cam.tool.length_missing`
(first calculation report of a toolpath that uses it) where the length is needed: a rotary
Setup's travel check, or a holder. The import warning tells the user early, once per tool.

**Holder.** A Fusion holder is a stack of segments, each with `height`, `lower-diameter` and
`upper-diameter`. The native `Holder` is one cylinder, `{ diameter, length }` (S 02 §2.4). The
importer takes the largest diameter over all segments and the sum of the heights. That is
conservative for collision checks (a cone is replaced by its widest cylinder). If any segment
is unusable (missing numbers, negative, over 10 000 mm) the whole holder is dropped with
`import.tool.holder_dropped`; no partial holder. At most 64 segments are read. Holder
entries that stand alone in a library (`type` = `holder`) are not tools and are skipped.

## 7. Vectric `.vtdb`: the SQLite decision

### 7.1 Decision

**Do not add `rusqlite` or any C SQLite.** When `.vtdb` import is built, read the file with a
small **read-only reader written for this purpose** in `fluut-post-import::sqlite`, in safe
Rust. No dependency, no FFI exception in S 01 §1.8.3, `#![forbid(unsafe_code)]` stays.

Reasons:

- The need is a read-only scan of a few tables of a file the user supplied. The SQLite file
  format (header, page types, B-tree table leaves and interior pages, varint, record format,
  overflow pages) is published by the SQLite project and is public domain. A reader that
  handles only rollback-journal files, table B-trees, text/integer/real/blob/null values and
  overflow chains is a few hundred lines.
- The input is untrusted (S 01 §1.7). A full SQL engine linked into the process is a large
  attack surface for a job that needs a table scan. A purpose-built reader is bounded by
  construction: page count, row count, record size and recursion depth all have caps (§8), and
  it never interprets SQL, triggers, views or extensions.
- `rusqlite` with `bundled` compiles the C amalgamation. It needs a C compiler on every build
  machine (the release workflow builds four targets), adds a build script, and would need an
  OQ-08 exception for one optional feature. The licences would pass `deny.toml` (MIT for
  `rusqlite`, public domain for SQLite), so licence is not the reason; the reasons are the
  attack surface, the C toolchain and the exception.
- `cargo deny` constraints (`deny.toml`: allowed licences, `multiple-versions = "warn"`,
  `wildcards = "deny"`, sources limited to the registry) are met by having no new dependency.

Not chosen, for the record: a pure-Rust SQLite crate. The ones known to the author are young or
write-capable; their maintenance and fuzzing record is not better than ours for the read-only
subset, and each is a new dependency to vet.

A WAL-mode file (a `-wal` side file) is read as its main file only, with a warning that
recent changes may be missing; the user is told to close Vectric first.

### 7.2 Status

Not built. The container reader is cheap, but without the table layout it reads nothing useful,
and writing it ahead of the mapping would be unused code.

### 7.3 What blocks the mapping

The tables and columns that hold Vectric tool geometry, tool groups, feeds and units are **not
publicly documented**, and the author may not use a proprietary sample to find out. The mapping
is therefore **not designed here**, and the CLI answers `.vtdb` with
`import.format.unsupported`. The way forward, in order:

1. A user lists the table and column **names** (not the data) of their own `.vtdb` with any
   SQLite viewer, and a short description of one tool they entered (size, type, feeds). This is
   their own data and may be put in an issue.
2. From that, a mapping table like §3 is written and fixtures are hand-built (a file made with
   the reader's own test writer, not a Vectric sample).
3. The reader and importer are built behind the same `fluut tools import` command, chosen by
   the `.vtdb` extension and a check of the SQLite header (`SQLite format 3\0`).

Expected mapping targets once known: tool type to shape as in §4 (V-bit, ball nose, end mill,
drill, engraving, form tools skipped), geometry in the unit the file declares (Vectric
databases mix inches and mm: the declared unit must be read, never assumed), and feeds as in
§3.3.

## 8. Validation and limits for untrusted input

The file is untrusted (S 01 §1.7, §1.8.1). No import path runs code, loads a plug-in, opens a
network connection, follows a path in the file, or reads another file.

| Limit | Value | Effect when exceeded |
|---|---|---|
| File size | 32 MiB | `import.file.too_large` (error); the CLI reads at most that many bytes plus one, so a huge file is not read whole |
| Tools per file | 10 000 | `import.limit.tools` (error) |
| Presets per tool | 200 | the rest dropped, `import.preset.dropped` |
| Holder segments per tool | 64 | the rest ignored |
| JSON nesting | 128 (the parser's limit) | `import.file.invalid` (error), no stack overflow |
| Name length | 200 characters | cut; control characters removed |
| A length | 0 to 10 000 mm | value dropped (tool skipped when it is the diameter) |
| A feed | at most 1 000 000 mm/min | dropped |
| Spindle speed | at most 1 000 000 rev/min | dropped |
| Numbers | finite only | dropped |
| Encoding | UTF-8, a leading BOM accepted | `import.file.invalid` |

The output is written by the importer's own TOML writer: strings are escaped (quotes,
backslashes, control characters), so a hostile description cannot add keys or tables to the
library (a test pins this). The result is then parsed by `ToolLibrary::parse`, the same parser
the program uses on load; the test suite checks every fixture round-trips through it. Output
is deterministic: no map with an unordered iteration reaches the file (D01 §6).

## 9. Diagnostics

All codes are in the `import` family. Errors stop the import and write nothing. Warnings let it
finish. The generated Diagnostics wiki page covers `param.*` and `cam.*`; the import codes are
listed on the [Import tools](../wiki/Import-Tools.md) page.

| Code | Severity | Meaning |
|---|---|---|
| `import.file.unreadable` | error | cannot read the input |
| `import.file.too_large` | error | over the size limit |
| `import.file.invalid` | error | not UTF-8, not JSON, or not a tool library (no `data` list) |
| `import.file.version` | warning | no library `version`; read as a plain tool list |
| `import.limit.tools` | error | too many tools |
| `import.library.bad_id` | error | the library id is not allowed |
| `import.format.unsupported` | error | the extension is not a supported format (`.vtdb` today) |
| `import.output.exists` | error | the output file exists and `--force` was not given |
| `import.output.unwritable` | error | cannot write the output |
| `import.tool.skipped` | warning | an entry that is not an object |
| `import.tool.unsupported_type` | warning | the type has no native shape (§4); tool left out |
| `import.tool.approximated` | warning | mapped to a similar shape, or `RE` missing on a bull nose (§4) |
| `import.tool.bad_value` | warning | a value was not usable and was dropped; when it is the diameter, the unit or the taper angle the tool is skipped |
| `import.tool.length_missing` | warning | no overall length (§6) |
| `import.tool.holder_dropped` | warning | the holder had unusable segments |
| `import.tool.id_duplicate` | warning | an id was made unique with a suffix |
| `import.tool.number_duplicate` | warning | the same tool number on several tools |
| `import.preset.dropped` | warning | a preset was dropped (duplicate name, over the limit, not an object) |
| `import.fields.dropped` | warning | one summary per file: how many source fields were not used, and the most common keys |

Every message names the entry (`tool 3 "6 mm flat": ...`). The command exits 1 for an error,
0 otherwise.

The import report (`ImportReport`) carries the counts: tools read, written and skipped, and
the dropped keys with counts. S 05 §5.10 asks for "mapped keys, unsupported features,
approximations and verification". The first three are the diagnostics above. **Verification**
for a tool library is the round trip: the CLI's own output always loads with `ToolLibrary::parse`
(tested), and the CLI prints the counts.

## 10. Entry points

### 10.1 CLI (built)

```
fluut tools import <file> -o <library.toml> [--id <library id>] [--name <text>] [--force]
```

`<file>` is a Fusion tool-library `.json`. The extension chooses the importer (`.vtdb` is
recognised and answered with `import.format.unsupported`). Warnings go to the error stream in
the usual `warning code: message` form, and one summary line to the output stream:
`<out>: 7 of 9 tools imported as library fusion/shop (2 skipped)`. Exit status: 0, 1 (error),
2 (bad command line). Put the output in a project's `tools/` folder or give it to the other
commands with `--tools`.

The command is a thin wrapper over `fluut_post_import::fusion::import(bytes, &ImportOptions)`,
which returns the library text, the warnings and the report. The GUI calls the same function (§10.2).

### 10.2 GUI (built 2026-11-11)

The ribbon's **Library** group has an **Import tools...** command, first in the group, enabled
when no job runs (a project is not needed). The Tools, Machines and Posts buttons beside it stay
greyed out until the library browser exists (Known limits), so the entry is a command of its own
and not an item of the unbuilt **Tools** menu. The command opens the system's **native file
dialog** (S 08, the `Dialogs` helper of **Import model**, kind `ImportToolsSource`) with filters
for `.json` and `.vtdb`. **There is no text box for a path anywhere in the ribbon.** A `.vtdb`
(or any other extension) is answered at once in the result window with
`import.format.unsupported`, and no second dialog opens. Otherwise a second native **Save**
dialog (`ImportToolsTarget`) asks where to put the library, offering the input's name with
`.toml` and starting in the project's `tools/` folder (else the project's folder). `.toml` is
added to a name without an extension.

The import runs on its own worker thread (`vm::tool_import`): it reads the file with the same
32 MiB cap as the CLI, calls `fluut_post_import::fusion::import` with the id `fusion/<slug of
the file stem>` and the stem as name (the same defaults as the CLI), and writes the file. The
frame polls the worker and repaints while it runs; a small window says so. The result window
shows the library id, the counts (read, written, skipped, source keys not used with a
collapsible list) and every warning with its code and severity, or, when nothing was written,
`Nothing was imported.` and the errors.

**Writing rules.** An import always makes a new file. The app cannot see the system dialog's own
"replace?" question, so it relies on it: a real Save dialog asks before it returns an existing
file, and the import then replaces it. Scripted dialogs (tests) do not confirm unless told to
(`Dialogs::script_confirms_overwrite`); an existing target that was not confirmed is refused with
`import.output.exists`, and the file is created with `create_new` so a file that appears
meanwhile is not replaced either. The library can never replace its own source
(`import.output.is_input`). A failed import writes nothing. Cancelling either dialog does nothing.

**Using the library.** Since 2026-11-11 a project can name tool library files outside `tools/`
(`tool_libraries`, D06 §7.2.1), and the session loads a changed list at once. With a project open,
the result window offers **Use in this project** for a library saved anywhere. For a file directly
in the project's `tools/` folder, which the project loads whenever it opens, the button only reads
the project's tool libraries again (`ReloadToolLibraries`, no document change, no undo step). For
any other file it adds a reference, relative to the project file where the two share a folder
(else absolute), as one `SetToolLibraries` edit: one undo step, and the project is marked as
changed. A file the project lists already is read again instead, so a library imported over an
earlier import shows its new tools. No library is copied. With no project open the window says to
open one. The earlier "Reload project to use it" button is gone.

Tests: view-model tests in `vm/tool_import/tests.rs` (the importer on the fixture, counts text,
overwrite, own source, `.vtdb` and other formats, bad JSON, missing file, unwritable target,
the phases) and window tests with scripted dialogs in `ui/tests/tool_import.rs` (happy path,
cancel at each dialog, `.vtdb`, bad JSON, overwrite with and without the dialog's confirmation,
reload with the library in the project's `tools/` folder, a library elsewhere, the command
disabled while a job runs). UAT: UAT-TLS-09 to 14. Screenshot: `import-tools-result.png`.

## 11. Layering

`fluut-post-import` is layer 5 in `xtask/layers.toml` (already listed). It depends on
`fluut-model` (layer 3: `Diagnostic`, and in tests `ToolLibrary`) and `serde_json`, which is
already a workspace dependency. `fluut-cli` (layer 8) depends on it. `check-layers` passes. No
dependency on `fluut-core`, so a future GUI wrapper may call it without the session.

The importer writes TOML text itself rather than serialising `ToolLibrary`, because the model
type derives `Deserialize` only (the model is read, never written, by the program). If a later
task makes tools editable and adds serialisation, the writer here becomes a call to it.

## 12. Tests

- Unit and integration tests in `crates/fluut-post-import/tests/fusion.rs` on a hand-written
  fixture, `tests/fixtures/fusion-sample.json` (nine entries: flat with a holder and two
  presets, an inch ball tool, bull, V, drill, tap, a holder entry, a tool with no diameter, a
  duplicate number and name).
- Every type in §4; inches converted to mm; presets and feeds; holder collapse; the missing
  overall length; duplicate ids and numbers; bad values dropped; a hostile name cannot break out
  of the TOML; every error (not JSON, no `data`, not UTF-8, 100 000-deep nesting, 10 001 tools,
  over the size limit, bad library id); bare array and BOM; unknown unit; deterministic output.
- Round trip: the output parses with `ToolLibrary::parse` with no warnings.
- CLI tests in `crates/fluut-cli/tests/tools_import.rs`: success and loadable output, refusal to
  overwrite, `--force`, bad JSON writes nothing, `.vtdb` and unknown formats, bad arguments (2),
  missing file.
- UAT: [UAT-Tools](../wiki/UAT-Tools.md), UAT-TLS-01 to 08.
- Not covered by an automated test: real Fusion files (none may be copied here). The first
  real file a user reports is to become a hand-reduced fixture.

## 13. Challenges

- **CH-26 (deferred) The Fusion JSON has no public schema.** The key table in §3 is unverified
  against an official document. Mitigation: only the listed keys are read, every value is
  checked, unknown keys are reported, and an unmapped tool type is skipped. Revisit when a user
  supplies a real file (hand-reduced) or Autodesk publishes the schema.
- **CH-27 (deferred) Vectric `.vtdb` layout unknown.** §7.3.
- **CH-28 (deferred) Preset-to-material matching.** Fusion presets are named freely; FluutCAM
  selects by material name. The importer keeps the preset name. A later Library UI may map
  names (for example "Aluminium" to a canonical material).
