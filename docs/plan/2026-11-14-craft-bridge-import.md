# Craft Bridge Import Plan (linked CAD models)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Open a model sent from SolidCraft as a *linked* model, and keep every face and edge pick when the design changes. FluutCAM reads SolidCraft's planned exchange file (`*.craftx`, `craft-exchange/1`, [SolidCraft RFC 0001](https://github.com/tangency-labs/solidcraft/blob/main/docs/rfcs/0001-craft-bridge.md)). Picks are stored by SolidCraft's persistent face names, which come from the design's feature history. A new revision re-finds every pick by name. A pick that can no longer be found is reported by name and stops the toolpath from being calculated or posted. It is never dropped.

**Why this route:** SolidCraft (MIT or Apache-2.0) already names every face from its kernel history (`F2:top`, `F4:side:l4`, `F2:top#[F2:side:l4,F4:side:l4]` for pieces, `F5:blend(<edge>)` for rounds, `<face A>|<face B>` for edges). That naming is the hard part of "update the design without losing references". Reading their exchange file gives FluutCAM the link through a file contract, with no code dependency on a codebase that changes daily. It also needs no change to FluutCAM's geometry kernel. The decision and the survey behind it were made on 2026-11-14 (SolidCraft, SolidCore and CamCraft compared; moving FluutCAM onto SolidCore, forking SolidCraft and writing our own CAD were rejected).

**State of the contract:** RFC 0001 is a draft (started 2026-11-07, Phase 0, nothing built). Its Phase 1, SolidCraft writing `.craftx`, does not exist yet. This plan therefore builds against the draft, with our own fixtures, and keeps every format detail in one reader module so a change to the RFC touches one place.

**Architecture:**
- `fluut-io-step` (layer 2): read the `name` of `ADVANCED_FACE` and `EDGE_CURVE` entities (today they are dropped). A STEP file that carries names is then matched by name for any CAD source, not only SolidCraft.
- `fluut-io-craftx` (new, layer 2, beside `fluut-io-step`): the `.craftx` ZIP reader. It checks the manifest, verifies each part's SHA-256, reads `bodies/<n>.step` through `fluut-io-step`, and attaches `faces.json` names, aliases and `exact` flags to the faces. It also reads `params.json` and `meta.json` for display. Untrusted input: limits on entry count, sizes and compression ratio, no paths outside the archive, unknown parts warned and skipped.
- `fluut-topo` (layer 1): an optional name table per B-rep body: `face_names: BiMap<FaceName, FaceKey>`, aliases, `exact` flag; the same for edges. `PersistId` of a named face becomes `hash(source_id, body key, name)` (D03 §6), so today's re-import matching keeps working unchanged for named faces.
- `fluut-model` (layer 3): a geometry source of kind `craftx` with its link `{source_id, revision, hash, path}` (S 02 §2.6); name-first re-resolution in `Session::reimport` (D06 §5.1) with the RFC's outcomes; a `broken` toolpath state.
- `fluut-core`, `fluut-cli`, `fluut-app`: change detection, the Update command, the link in the Model panel, the `fluut model update` command.

**Design:** [S 02 §2.6](../spec/02-data-formats.md), [S 03 §3.5](../spec/03-data-model.md), [D03 §6](../design/D03-topology-bvh.md), [D04](../design/D04-step-reader.md), [D06 §5.1](../design/D06-project-model.md), [S 08 §8.6](../spec/08-user-interface.md).

## Global Constraints

All earlier Global Constraints apply. In addition:

- No SolidCraft, SolidCore or CamCraft source is copied. The format is implemented from RFC 0001's text; face names are opaque strings to FluutCAM (the RFC asks receivers to treat them so).
- No new crate dependency without a `cargo deny check` pass. ZIP reading uses `zip` with only the deflate feature, or `flate2` (already in the tree) with a small reader of our own; the choice is made in Task 2, step 1.
- No path boxes in the ribbon: opening a `.craftx` uses the native file dialog (an extra filter on Import model).
- Every commit passes `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo xtask check-layers`, `cargo deny check` and `cargo xtask wiki-reference --check`.

## Task 0: the contract (needs the owner)

- [ ] **Step 1:** Decide, with the owner, whether to comment on RFC 0001 as a second CAM receiver. Posting is public and needs the owner's go-ahead.
- [ ] **Step 2:** If yes, draft the comment for the owner to approve. Points to raise:
  - **Face index.** `faces.json` gives "each face's index", but the RFC does not say what the index counts. FluutCAM proposes the order of `ADVANCED_FACE` entities in `bodies/<n>.step`, and also writing the persistent name into each `ADVANCED_FACE` name in the STEP. Then the name survives even when the face table and the STEP are read apart.
  - **A point on each face.** `model.faces` already lists one. Carrying it (and the face's area) in `faces.json` lets a receiver check the face index by geometry.
  - **Edges.** State whether edges are listed in `faces.json` or derived from face names (`<face A>|<face B>`, `~k`).
  - **Split pieces.** Say how pieces of one name are listed (one entry per piece, with its qualifier).
  - **`requires`.** List `faces.names/1` there, so a reader that cannot use names does not silently fall back to geometry.
  - **Two receivers.** The contract should stay app-neutral, with no field that only CamCraft reads.
- [ ] **Step 3:** Record the answers (or their absence) in this plan; Task 1 fixtures follow the draft until then.

## Task 1: fixtures and the reader's model of the format

- [ ] **Step 1:** Write `craftx` fixtures by a test helper, not by hand-zipped files. The helper takes one of our STEP fixtures, a face-name table and a revision, and writes the ZIP with manifest, `bodies/0.step`, `faces.json` and `params.json`. Fixtures:
  - a block with a pocket (revision 1);
  - the same block with a hole added: names keep their faces, the face count changes and faces move in the file;
  - a fillet added on one edge: an edge pick is lost, a `blend` face appears;
  - the pocket face split in two by a rib (`#[…]` pieces);
  - two faces merged (an alias);
  - a face marked `exact: false`.
- [ ] **Step 2:** One module (`fluut_io_craftx::format`) holds every field name and version rule of `craft-exchange/1`. The reader accepts any minor version, refuses an unknown major, warns on unknown parts, and refuses a file whose `requires` lists a part it cannot read.

## Task 2: reader

- [ ] **Step 1:** Choose the ZIP reader (`zip` without default features, or `flate2` with our own reader); run `cargo deny check`.
- [ ] **Step 2:** Archive safety: at most 1,000 entries, 1 GiB unpacked, a compression ratio cap, entry names without `..`, absolute paths or drive letters, and each part's SHA-256 checked against the manifest. Errors and warnings are `import.craftx.*` diagnostics (`advice.toml`, so the wiki reference is generated).
- [ ] **Step 3:** Read the manifest, the bodies (through `fluut-io-step`), the face table and the parameters. Attach names to faces by the face index of Task 0; check each face against its point (when given) within the modelling tolerance, and mark a face whose point misses as `exact: false` with a warning.
- [ ] **Step 4:** Unit tests: each fixture; every refusal; a tampered part hash; a ZIP slip entry; an oversized entry.

## Task 3: names in the B-rep and in STEP

- [ ] **Step 1:** `fluut-io-step` keeps `ADVANCED_FACE` and `EDGE_CURVE` names (empty names stay absent). D04 updated.
- [ ] **Step 2:** `fluut-topo` name tables for faces and edges, with aliases and the `exact` flag; `PersistId` from the name when present (D03 §6 updated). Bodies without names keep today's ids, so existing projects open unchanged (a test opens every project fixture and compares ids).
- [ ] **Step 3:** Picks store the name beside the id in the project file: `{ face = "f-12", name = "F2:top" }`, `{ edge = "e-3", name = "F2:top|F4:side:l4" }`. Older readers ignore the extra key (check S 02's rule for unknown keys, and add it if it is missing).

## Task 4: re-resolution with named outcomes

- [ ] **Step 1:** Extend `Session::reimport` (D06 §5.1): for named picks, the name first, then today's signature matching. Outcomes, as RFC 0001 §4 states them:
  - `exact`: the name is there; the pick is kept.
  - `merged`: another face answers to the name as an alias; the pick is kept and a note given.
  - `split`: several pieces carry the name; the pick becomes all the pieces, with a warning, because for a pocket floor or a contour that can change the meaning.
  - `by_geometry`: the name matched only a face marked `exact: false`; the pick is kept and the toolpath flagged "check this".
  - `missing`: the toolpath becomes `broken`.
- [ ] **Step 2:** A `broken` toolpath keeps its last result for viewing. Calculation and posting refuse it, naming the face, its last place and the toolpath. Re-picking clears it. S 03 §3.5 is strengthened from "reported" to "reported and blocks posting" for linked models.
- [ ] **Step 3:** Edge picks resolve as the edge between their two named faces, so an edge survives a split of one of its faces.
- [ ] **Step 4:** Tests: one per outcome, on the Task 1 fixtures; a project round trip; undo of an update as one edit group.

## Task 5: change detection and Update

- [ ] **Step 1:** `fluut-core` checks a linked file's hash when the window gains focus and every 2 seconds while it has focus (no file-watcher crate). A new hash raises "Linked model changed: Update". Nothing recalculates on its own.
- [ ] **Step 2:** Update re-reads the file, re-resolves the picks (Task 4), marks affected toolpaths stale and shows the outcomes in one report.
- [ ] **Step 3:** `fluut model update <project> [--model <id>]` prints the same report; exit 0 when every pick resolved, 1 when any is broken, 2 on error. The script function `model.reimport` returns the same outcomes.
- [ ] **Step 4:** The Model panel shows the link: source app, revision, saved time, `params.json` values (read only), and Update. Known-Limits' "re-import is a script function" is removed once the window command exists.

## Task 6: launching from SolidCraft

- [ ] **Step 1:** `fluutcam <file.craftx>` and `fluutcam --open <file.craftx>` open the file in a new project, or offer to update the matching linked model when a project already links that `source_id`.
- [ ] **Step 2:** A running window takes the file over instead of starting a second one. This is deferred if it needs more than a lock file and a loopback hand-off already in the tree.

## Task 7: optional interim exporter (local only, needs the owner)

Until SolidCraft ships `bridge.export`, a local script can make a `.craftx` from a `.sc` design with `solidcraft-cli`: run a batch that opens the design, exports STEP and reads `model.faces`, then zip them in the draft format. This tests real designs before Phase 1 lands. Installing SolidCraft is a download, so the owner decides. The script is not shipped and is deleted once SolidCraft writes the file itself.

## Task 8: documentation and UAT

- [ ] **Step 1:** Spec and design: S 02 §2.6 (the `craftx` source and the pick `name` key), S 03 §3.5, D03 §6, D04, D06 §5.1.
- [ ] **Step 2:** Wiki: a new `Linked-Models.md` (send from SolidCraft, update, what each outcome means, how to re-pick), `Import-a-Model.md`, `Known-Limits.md`, `Troubleshooting.md`.
- [ ] **Step 3:** UAT: cases in `UAT-Import.md` for each fixture outcome, and one end-to-end case with a real SolidCraft design once Task 7 or SolidCraft's Phase 1 exists.

## Open questions

1. Will SolidCraft accept the face-index definition and STEP face names (Task 0)? Until then the reader checks names by point.
2. Should FluutCAM also send something back, such as the stock body or as-machined body? RFC 0001 leaves this open; FluutCAM sends nothing in this plan.
3. Exact B-rep through `bodies/<n>.record` (SolidCore's record format, RFC Phase 4): worth reading only if it carries geometry STEP loses. Not planned until the format is published and stable.
4. Live updates over SolidCraft's control channel (RFC transport B): not planned; polling the file is enough for one user on one machine.
