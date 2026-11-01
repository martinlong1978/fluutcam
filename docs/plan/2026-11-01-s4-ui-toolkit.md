# S4 — UI Toolkit Spike Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the product owner the evidence to decide OQ-09: `egui` or `slint` for `fluut-app`. Exit criterion from the roadmap: decision recorded with frame times and code size; `slint` licence checked. This spike gives the numbers, the licence facts and a recommendation. The product owner decides.

**Architecture:** Three crates in `spikes/`, each with its own `[workspace]` like `spikes/s1-truck`, so nothing reaches the product, `cargo deny` or the layer check:
- `spikes/s4-common`: what both prototypes share and what is toolkit-independent in D13. A generated mesh, an orbit camera, the `wgpu` mesh renderer (draws into a texture), the brand colours, mock Setup/Toolpath data and the measurement harness.
- `spikes/s4-egui`: `eframe` + `egui_dock`. The viewport texture is shown through `egui_wgpu`.
- `spikes/s4-slint`: `slint` with the `wgpu` renderer. The viewport texture is imported with `slint::Image::try_from(wgpu::Texture)`.

Nothing under `crates/` changes.

**Tech Stack:** `wgpu` 30 (the version both toolkits support); `eframe`/`egui` 0.36 with `egui_dock` 0.21 (which needs 0.36, not 0.37); `slint` 1.18 with `unstable-wgpu-30`; `glam`, `bytemuck`, `pollster`.

**Design:** [`../design/D13-ui-architecture.md`](../design/D13-ui-architecture.md) §1, §4–§6, §9, §10; [`../spec/08-user-interface.md`](../spec/08-user-interface.md) §8.2–§8.5, §8.9, §8.10a, §8.11; [`../design/D15-toolpath-parameters.md`](../design/D15-toolpath-parameters.md) §4 (mock fields); [`../spec/06-security-and-distribution.md`](../spec/06-security-and-distribution.md) §6.1; [`../design/challenges.md`](../design/challenges.md) CH-02, CH-23.

## Global Constraints

All earlier Global Constraints apply to the workspace. In addition:

- Spike crates are outside the root workspace. Every commit still passes `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo xtask check-layers` and `cargo deny check` at the root. The spike crates also pass `cargo fmt` and `cargo clippy -D warnings` in their own directories.
- Both prototypes have the same features, the same mock data and the same renderer, so the differences measured are the toolkit's.
- Brand colours from `assets/README.md`: ink `#16181D` (base surface), cutter orange `#FF5B14` (accent, selection), stock grey `#8A93A0` (model), paper `#F4F2EE` (text), flute white `#FAF9F6`.
- Measurements run on a headless cloud machine: Xvfb with Mesa's software Vulkan driver (lavapipe), 4 cores. GPU frame times there are CPU rasteriser times. What that machine cannot measure is listed in the report, not guessed.
- No content derived from DeskProto.

## Review Focus

1. **Same work in both.** The two prototypes show the same panels and fields, and the bench mode does the same camera motion, so the numbers compare.
2. **Measurement method.** Each number says how it was taken and what it includes.
3. **Licence facts.** Each claim about a licence links its source text.

---

### Task 1: Plan

**Files:** Create `docs/plan/2026-11-01-s4-ui-toolkit.md`; modify `docs/plan/README.md`.

- [x] **Step 1:** Write this plan; add it to the README table; commit.

### Task 2: Shared viewport and mock data

**Files:** Create `spikes/s4-common/{Cargo.toml, src/lib.rs, src/mesh.rs, src/camera.rs, src/render.rs, src/mock.rs, src/bench.rs, src/shader.wgsl}`.

- Mesh: a relief height field on an `n × n` grid (`2 (n−1)²` triangles, `n = 708` for about 1 M), smooth normals, `u32` indices.
- Camera: orbit (yaw, pitch about a target), pan (move target in the view plane), zoom (distance, toward the cursor not needed).
- Renderer: colour and depth textures of a given size; gradient background; soft two-light shading; a draped pass preview: raster lines at `stepover` and `pass_angle_deg` drawn on the mesh in the fragment shader, as CH-23's workaround proposes.
- Mock data: two Setups with toolpaths (`pocket_2d`, `contour_2d`, `parallel`) and their status icons (S 08 §8.6). The `pocket_2d` inspector has D15 §4.1 `passes` fields (`pattern`, `stepover`, `step_down`, `direction`, `offset_order`, `pass_angle_deg`) and D15 §4.2 validation of `stepover` against the tool diameter.
- Bench harness: `S4_BENCH=<frames>` orbits the camera every frame and prints frame-time percentiles; `S4_STARTUP=1` prints the time to the first presented frame and exits.
- [x] **Step 1:** Unit tests: triangle count, normals unit length, camera orbit keeps distance, pan moves the target, validation codes.
- [x] **Step 2:** Implement; pass.

### Task 3: `egui` prototype

**Files:** Create `spikes/s4-egui/{Cargo.toml, src/main.rs}`.

- Title bar with units badge; ribbon with stage tabs and a toolpath gallery (S 08 §8.2–§8.3); browser tree; inspector with a scrub-able numeric field and the validation message under it; viewport with an overlay work-zero triad; panels in `egui_dock`, so they dock and float.
- [x] **Step 1:** Run under Xvfb; take a screenshot for the report.

### Task 4: `slint` prototype

**Files:** Create `spikes/s4-slint/{Cargo.toml, build.rs, src/main.rs, ui/app.slint}`.

- The same panels and fields. Slint has no docking; panels are fixed in a layout.
- [x] **Step 1:** Run under Xvfb; take a screenshot for the report.

### Task 5: Measurements

**Files:** Create `spikes/s4-common/measure.sh`.

For each prototype: frame time while orbiting about 1 M triangles, and with a small mesh to separate UI cost from mesh cost; idle CPU over 10 s; startup time to the first frame; release binary size (as built and stripped); lines of code (toolkit-specific only); clean release build time.

- [x] **Step 1:** Run; record the numbers and what the machine cannot measure.

### Task 6: Licences

- `slint`: GPL-3.0, the royalty-free licence and the commercial licence, against MIT OR Apache-2.0 and the shipped binaries of D14 §5 and S 06 §6.1–§6.2. `egui` and the crates it brings.
- [x] **Step 1:** Record facts with links to the licence texts.

### Task 7: Accessibility, IME, localisation and high DPI

- What each toolkit provides for S 08 §8.9, §8.10a and §8.11 items 5–6, checked in the prototypes where the headless machine allows and in the sources otherwise.
- [x] **Step 1:** Record findings.

### Task 8: Report and challenges

**Files:** Create `docs/plan/spikes/s4-ui-toolkit.md`; modify `docs/design/challenges.md` (CH-02, CH-23), `docs/plan/README.md`.

- [x] **Step 1:** Report: numbers, licence findings, the S 08 §8.11 checklist, and a recommendation marked as for the product owner to decide.
- [x] **Step 2:** Update CH-23 and CH-02; mark this plan done; commit and push.

**Done.** Findings in [`spikes/s4-ui-toolkit.md`](spikes/s4-ui-toolkit.md). The recommendation (egui) is for the product owner to decide; S 01 §1.8, S 08 §8.11 and D13 §6 change only after that decision. Deviations from this plan:
- Slint was also measured with its Skia renderer (`--features skia`), because FemtoVG on the software rasteriser hid the toolkit's own cost.
- `spikes/s4-common/a11y/` was added to read each prototype's AT-SPI tree.
- The bench harness gained `S4_TREE` (a 200-toolpath browser) and `viewport_bench` gained `S4_PREVIEW` (preview cost).
- Docking was tested only by moving a tab between tab groups in egui. Native extra windows were not prototyped in either toolkit.
