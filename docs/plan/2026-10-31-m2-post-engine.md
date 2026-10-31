# M2 — Post Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `fluut-post` turns `fluut-path` toolpath results into G-code through layered, declarative posts (spec 05, D10). Built-in posts `fluut/gcode`, `fluut/grbl` and `fluut/linuxcnc` have golden output for the reference toolpath suite.

**Architecture:** The generic layered resolver (D06 §4.3) is the first module of a new layer-3 crate `fluut-model` (`defs`), because machines and templates use it too. `fluut-post` (layer 4) adds the post block merger, typed post, validation, expressions, templates, number formatting and the emitter. Until `fluut-model` has project types, the emitter takes its own input struct (`PostJob`) that `fluut-core` will fill later. Importers (`fluut-post-import`) are not in M2: they wait for the Vectric samples of CH-21 (M7).

**Tech Stack:** as M1; adds `toml` 1 and `serde` 1 (`derive`), `serde_ignored` (unknown-key warnings, spec 02 §2), `sha2` (resolved-post hash, spec 05 §5.9).

**Spec:** [`../spec/05-postprocessor-engine.md`](../spec/05-postprocessor-engine.md), [`../spec/02-data-formats.md`](../spec/02-data-formats.md) §2.2–2.3, [`../design/D10-post-engine.md`](../design/D10-post-engine.md), [`../design/D06-project-model.md`](../design/D06-project-model.md) §4.

## Global Constraints

All M0 and M1 Global Constraints apply. In addition:

- Rounding happens only in `fluut-post` (spec 05 §5.8). Every coordinate word comes from the exact `f64` endpoint; incremental deltas are between rounded absolutes.
- Number formatting is decimal-exact: the shortest round-trip decimal of the scaled `f64` (Rust's `{:e}` formatting, which is shortest round-trip) is rounded half away from zero as a decimal string. No `ryu` dependency (D10 §4.3 named it; std gives the same digits).
- Evaluation of post expressions never fails at run time; all errors are found when the post is compiled.
- Post output is deterministic: no hash-map iteration order in output, `libm` for trigonometry, date and time come from `PostJob`, not the clock.
- Missing parent or mixin is an error; `extends` cycle is an error; depth limit 16 (spec 05 §5.2.2).

## Review Focus

1. **Rounding edge cases**: `…5` at the last decimal (0.0005 at 3 decimals → 0.001; −0.0005 → −0.001), tiny negatives (−0.00001 → `0`, never `-0`), large values (1e5 with 4 decimals). Tests in Task 5.
2. **Arc centre correction** (spec 05 §5.8 rule 3): random arcs and decimals either pass the controller radius check after rounding or are linearized. Property test in Task 7.
3. **Patch merge errors**: unknown line id in `replace_line`/`remove_line`/`before`/`after` and duplicate ids are errors naming the layer. Tests in Task 2.
4. **Modal suppression** never drops a line that carries a changed word or a non-motion literal (e.g. `M3`). Tests in Task 6.
5. **Full-circle arcs** (a == b) emit as two half arcs or one 360° arc per `max_sweep_deg`; never a zero-length arc. Tests in Task 7.

---

### Task 1: `fluut-model` crate with the generic definition resolver

**Files:** Create `crates/fluut-model/{Cargo.toml, src/lib.rs, src/defs.rs, src/defs/tests.rs}`; add `fluut-model = 3` (already in `xtask/layers.toml`) to the workspace.

**Interfaces:**
- `DefRef { id: String, version: VersionReq }`, `enum VersionReq { Latest, Major(u32), Exact(u32, u32) }`, `DefRef::parse("fluut/linuxcnc@3.2")`.
- `Version { major: u32, minor: u32 }` parsed from `version = "3.2"`.
- `trait DefStore { fn lookup(&self, r: &DefRef) -> Result<Def, DefError>; }`; `Def { id, version: Version, table: toml::Table }`; `MemStore` (in-memory, picks the highest matching version).
- `trait ModeMerger { fn merge_mode(&self, path: &str, parent: Option<&toml::Value>, child: &toml::Table, layer: &LayerId) -> Result<toml::Value, DefError>; }` called for tables with a `mode` key.
- `fn resolve(store, root: &DefRef, merger) -> Result<Resolved, DefError>`; `Resolved { table: toml::Table, layers: Vec<LayerId>, provenance: BTreeMap<String, LayerId>, warnings: Vec<DefWarning> }`. `extends`, `mixins`, `id`, `version`, `format` are not merged into the result.
- `fn apply_overrides(table, overrides: &toml::Table) -> Result<, DefError>` for dotted-path keys (spec 05 §5.2.3).
- `enum DefError { NotFound(DefRef), Cycle(Vec<String>), TooDeep, BadRef(String), BadVersion(String), Merge{layer, path, msg} }`; `DefWarning::MixinConflict{path, earlier, later}`.

- [x] **Step 1:** Tests: `parse_refs` (latest, `@3`, `@3.2`, bad forms); `store_picks_highest_matching`; `deep_merge_tables_scalars_arrays`; `extends_chain_three_deep`; `mixins_apply_in_order_with_conflict_warning`; `missing_parent_error`; `cycle_error`; `depth_limit`; `provenance_records_layer`; `mode_table_goes_to_merger`; `overrides_by_path` (including creating a missing leaf, rejecting a path through a non-table).
- [x] **Step 2:** Fail, implement, pass.
- [x] **Step 3:** Workspace checks.
- **Done.** Deviation from D06 §4.3: a mixin is not resolved on its own and then merged; its layers (its own parent chain, its mixins, its keys) are applied onto the result so far. Otherwise a mixin's `append` or `patch` block would act on an empty block and lose the parent's lines. Provenance is per leaf path; a mode table (post block) is one leaf, so per-line provenance comes with the post merger in Task 2. `DefStore::lookup` returns `Arc<Def>`; `NoModes` is the merger for kinds without mode tables.

### Task 2: `fluut-post` crate, block merger, flatten

**Files:** Create `crates/fluut-post/{Cargo.toml, src/lib.rs, src/merge.rs}`

**Interfaces:**
- `PostMerger: ModeMerger` implementing D10 §2 for `blocks.*`: `replace` (default), `append`, `prepend`, `remove`, `patch` (`replace_line`, `remove_line`, `before`, `after`). Lines are strings or `{ id?, when?, line }` tables; merged blocks are stored as `{ lines = [...] }` with ids kept.
- `fn resolve_post(store, r: &DefRef, overrides: Option<&toml::Table>) -> Result<Resolved, DefError>`.
- `fn flatten(resolved: &Resolved, id, version) -> String` (standalone TOML, no `extends`/`mixins`/`mode`).
- [x] **Step 1:** Tests: each mode; patch with every operation; unknown id error; duplicate id error; `append` to a missing parent block equals its lines; `flatten` round trip (resolving the flattened post gives the same table).
- [x] **Step 2:** Fail, implement, pass.
- **Done.** Every `blocks.<name>` table goes to `PostMerger` (new `ModeMerger::handles` hook), so each merged line records its `layer` for `explain`; `flatten` drops it. `mode` on any other table is an error.

### Task 3: Typed post and validation

**Files:** Create `src/post.rs`, `src/validate.rs`
- `Post` deserialised from the resolved table with serde: `name`, `output { extension, units, line_ending, decimal_char, substitute, max_file_size, max_segment_length, comment_start, comment_end }`, `line_numbering { enabled, start, increment, maximum, prefix }`, `words: BTreeMap<String, WordFmt>` (spec 05 §5.3 keys plus `modal`), `blocks: BTreeMap<String, Block>`, `arcs`, `helix`, `rotary`, `spline`, `cycles`, `feed { rotary_method }`. Defaults per spec 05 §5.5.
- Unknown keys collected with `serde_ignored` as warnings (spec 02 §2).
- Validation (spec 05 §5.2.5): unknown block names; `arc_cw`/`arc_ccw` required when `arcs.enabled`; `centre_mode = "r"` with `max_sweep_deg > 180`; `min_radius > max_radius`; cycle kinds unknown. Placeholder checks happen in Task 5 when templates compile.
- [x] **Step 1:** Tests: defaults applied; unknown key warning names its path; each validation error.
- [x] **Step 2:** Fail, implement, pass.
- **Done.** `Post::load(&Resolved) -> Result<LoadedPost { post, warnings }, PostError>`. No `modal_codes` table: modal behaviour is per word (`modal`). Also required: `rapid` and `feed` blocks in every post; a `comment` block and `first_*` variants of motion events are known block names. Unknown keys are warnings (spec 02 §2.0), not errors.

### Task 4: Expression language

**Files:** Create `src/expr.rs`, `src/expr/tests.rs`
- Lexer and recursive-descent parser for the EBNF in spec 05 §5.4.3; `enum Ty { Num, Str, Bool }`; `Schema` (path → type, D10 §3.2 roots `tool`, `machine`, `setup`, `toolpath`, `state`); `fn compile(src, &Schema) -> Result<Expr, ExprError>` (type-checked; error carries the column); `fn eval(&Expr, &dyn Context) -> Value` (total: division by zero gives 0, `round` half away from zero).
- [x] **Step 1:** Tests: precedence; string and number comparison; `!`, `&&`, `||`; functions and arity errors; unknown path; type mismatch (`tool.number == 'x'`); division by zero is 0; proptest: random token strings never panic in `compile`.
- [x] **Step 2:** Fail, implement, pass.
- **Done.** Types are checked while parsing. `<`/`>` compare numbers only; `+` is numeric only (no string concatenation). `round(x, n)` takes an optional decimal count. A context value that is missing or of the wrong type evaluates as its type's default (0, '', false). `compile_condition` requires a boolean.

### Task 5: Number formatting and templates

**Files:** Create `src/format.rs`, `src/template.rs`
- `fn format_number(v: f64, f: &NumFmt) -> String` per D10 §4.3 (`decimals`, `scale`, `sign`, `trailing_zeros`, `leading_zero`, `pad_width`, `decimal_char`); `fn round_dec(v, decimals) -> f64` used by the emitter for rounded positions.
- Template tokens `Lit | Word | Placeholder` (D10 §3.1); `compile_line(&str, &Post) -> Result<Line, PostError>` resolves `[NAME]` to a word, else a built-in placeholder (spec 05 §5.4.2), else an error naming block and line. Conditional lines compile their `when` expression.
- [x] **Step 1:** Tests: rounding cases in Review Focus 1; `-0` never printed; leading-zero and trailing-zero options; decimal comma; padding; unknown placeholder error; template tokenisation of `"G1 [X][Y] F[F]"`.
- [x] **Step 2:** Fail, implement, pass.
- **Done.** `pad_width` is the minimum count of integer digits (zero-padded, e.g. `T01`); `sign = "always"` puts `+` on zero and positive values; with `trailing_zeros = false` a whole number has no decimal point. `[NAME]` is a token only when `NAME` is ASCII letters, digits and `_`; other brackets are literal. `CompiledPost::compile(LoadedPost)` compiles every block and reports all template errors together; it also adds `[COMMENT]` and `[DWELL]` placeholders for `manual_nc`.

### Task 6: Emitter core

**Files:** Create `src/job.rs`, `src/emit.rs`, `src/emit/tests.rs`
- `PostJob { program_name, date, time, machine: MachineInfo, setups: Vec<SetupJob> }`; `SetupJob { name, kind (Fixed | Indexed | Rotary), work_offset, index_a, index_b, toolpaths: Vec<ToolpathJob> }`; `ToolpathJob { name, tool: ToolInfo, spindle_rpm, coolant, feeds: Feeds (FeedRef → mm/min), safe_z, result: Arc<ToolpathResult> }`.
- `fn post(job: &PostJob, post: &CompiledPost) -> Result<PostOutput, PostError>`; `PostOutput { files: Vec<String>, line_map: Vec<LineMapEntry>, warnings }`.
- Event loop of spec 05 §5.6 for header, footer, setup and toolpath blocks, tool change, spindle and coolant on/off, feed change, rapid / first_rapid / feed / plunge / retract classification (D10 §4.2); modal word suppression (`when = "changed"`) and dropping lines that become a bare motion code already in effect (D10 §3.1); line numbering; line ending; provenance comment block (spec 05 §5.9) with resolved-post hash.
- [x] **Step 1:** Tests: minimal program (header, one rapid, one feed, footer) exact text; repeated feed moves suppress unchanged words; `M3` line with no words never dropped; tool change fires only on change; line numbers; CRLF output; line map covers each motion line.
- [x] **Step 2:** Fail, implement, pass.
- **Done.** Extra rules found while implementing:
  - A suppressed word also removes letters written directly before it and one space (`F` in `F[F]`), so `G1 X[X] Y[Y] F[F]` with unchanged Y and F reads `G1 X10`.
  - Safety rule: after any non-motion block writes a line, the last emitted axis words (X Y Z A B I J K) and motion code are forgotten, so the next move is written in full. A tool-change block may move the machine (`G53 G0 Z0`).
  - Line numbering skips lines starting with `%`.
  - Spindle and coolant are stopped before a tool change and before the footer.
  - Arcs, helices and splines are linearized until Task 7; rotary moves, cycles and markers return `EmitError::Unsupported` until Task 8.
  - Inch output divides lengths and feeds by 25.4. `PostJob.provenance` (`Provenance::of(&Resolved, id)`, SHA-256 of the resolved table) fills the first comment block.

### Task 7: Arcs and helices

**Files:** Create `src/arcs.rs`; modify `src/emit.rs`
- Plane detection (normal ±Z, ±Y, ±X within 1e-9) and CW/CCW from sweep sign seen from the plane normal (G17 +Z, G18 +Y, G19 +X); acceptance (`arcs.enabled`, `planes`, radius limits); splitting above `max_sweep_deg` into equal parts; centre correction (D10 §4.4) with one-more-decimal retry up to 6, else linearize; `[I] [J] [K]`, `[IA] [JA] [KA]`, `[R]` (negative for > 180°), `[PLANE]`. Helices as arcs plus the axial word, when `helix.enabled` and plane allowed.
- [x] **Step 1:** Tests: quarter arcs in each plane and both directions; full circle per `max_sweep_deg` (360 and 180); R mode sign; arc rejected by radius limits is linearized within its tolerance; property test (Review Focus 2).
- [x] **Step 2:** Fail, implement, pass.
- **Done.** The rounded start S′ is the position the controller holds: the value last written for that axis word, or the start rounded at the word's decimals after the safety rule. Raising decimals (up to 6) applies to that arc's in-plane axis and centre words only. An arc whose rounded chord vanishes (not a full circle) is linearized. Arcs with a rapid feed and arcs in non-principal planes are linearized. `[R]` is negative above 180°, which validation only allows outside `r` mode, so in practice R is positive. `[Radius]` is the exact radius, `[PLANE]` is `G17`/`G18`/`G19`.

### Task 8: Rotary, cycles, markers, cutter compensation, limits, file split

**Files:** Modify `src/emit.rs`; create `src/cycles.rs`
- Rotary moves with `[A]`/`[B]`, split above `max_angle_per_line`; rotary feed `degrees` / `inverse_time` / `linear_only` (spec 05 §5.7, G93/G94 modal); indexed setup blocks (`rotary_unclamp`, `rotary_index`, `rotary_clamp`).
- Cycles: `cycle_start` / `cycle_point` / `cycle_end` with `[CYCLE_Z] [CYCLE_R] [CYCLE_Q] [CYCLE_P] [CYCLE_F]`; expansion when unsupported (D10 §5 table; tap without a spindle-reverse block is an error).
- Markers to `manual_nc` (`[COMMENT]`, `[DWELL]`); `comment` block; cutter compensation events fired by passes marked compensated (field added to `fluut-path::Pass` as `comp: Option<CompSide>`).
- `max_segment_length` splitting; `max_file_size` splitting at the first retract after the limit.
- [x] **Step 1:** Tests: rotary feed values for each method; split rotary; each cycle kind native and expanded; markers; comp events; segment split; file split lands after a retract.
- [x] **Step 2:** Fail, implement, pass.
- **Done.** Decisions made while implementing:
  - Inverse-time feed writes `G93` before the first rotary line and `G94` before the next non-rotary move; these two codes come from the engine, not a block. After rotary moves the next linear move always states F.
  - While a canned cycle is open, rapid links at or above its R plane are held back: the next `cycle_point` makes them unnecessary; anything else ends the cycle (`cycle_end`) and then writes them.
  - Expanded peck and chip-break cycles retract 0.5 mm (at most half the peck) above the last depth before the next feed. Expanded tap is an error.
  - `manual_nc` lines test `marker.kind` (`stop`, `optional_stop`, `comment`, `dwell`, `pass_through`); `[COMMENT]` holds comment or pass-through text, `[DWELL]` seconds. Without a `manual_nc` (or `dwell`) block the item is skipped with a warning.
  - Cutter compensation events surround each pass with `Pass::comp` set (new `fluut_path::CompSide`).
  - A file split happens at a pass boundary after a rapid or retract, never inside a cycle or a compensated pass. Each file is a whole program: provenance, `header`, spindle and coolant restored, …, `footer`.

### Task 9: Reference suite, built-in posts, golden files

**Files:** Create `src/reference.rs`, `src/builtin.rs`, `posts/fluut/{gcode,grbl,linuxcnc}.post.toml`, `tests/golden.rs`, `tests/golden/*.nc`
- Reference suite (D10 §8) built in code: lines in all axes; arcs in XY/XZ/YZ with sweeps 1°, 90°, 179°, 181°, 359°, 360°; radii 0.01 and 2000; helices; rotary with and without X; tool changes; feed and spindle changes; each cycle kind; markers; an indexed setup; rounding edge values.
- Built-in posts embedded with `include_str!` and served by a `BuiltinStore: DefStore`; a SHA-256 of each file is checked at load against a table generated at build time (spec 05 §5.9).
- Golden test posts the suite with each built-in post and compares text; `FLUUT_BLESS=1` rewrites the files. Golden output is hand-checked before the first commit.
- [ ] **Step 1:** Write posts and suite; bless; read every golden file by eye against RS-274/NGC, Grbl 1.1 and LinuxCNC documentation.
- [ ] **Step 2:** Workspace checks; update `plan/README.md`.
