# GPU Frame Times Plan

**Goal:** Measure startup and frame time on a real GPU. The [M5 plan](2026-11-01-m5-ui-shell.md) measured them only on a cloud VM with a CPU rasteriser (lavapipe), and [D13 §5.4](../design/D13-ui-architecture.md) lists what needs a GPU: the 16 ms frame at 2 M segments, upload, pick latency and cold start.

**Architecture:** `fluut-app` only. `bench.rs` gains a header line (adapter, backend, driver, window size, vsync, simulation), p99, the CPU time of each frame, and two environment variables. No change to the renderer.

**Design:** [D13](../design/D13-ui-architecture.md) §5.4.

## Method

- Machine: Intel Core i7-8700, AMD Radeon Pro W5700 (driver 30.0.13018.2, "AMD proprietary driver", wgpu backend Vulkan, discrete GPU), Windows 11 Pro. A shared machine, so each case was run three times and the range is given. The session was a disconnected remote-desktop session whose display is the virtual `WinDisc` one (see "vsync" below).
- Release build: `cargo build --release -p fluut-app --bin fluutcam`. Window 1440 × 900 points, which is 2160 × 1350 px at the display scale of 1.5 (the header line prints it).
- Orbit: `FLUUT_BENCH=200` opens and calculates the project, then orbits the camera 6 px a frame and times 200 frames after 20 warm-up frames. `frame` is the interval between frame starts, `cpu` is eframe's CPU time of a frame (UI, tessellation and encoding, without waiting for vsync).
- `FLUUT_BENCH_SIM=1` also simulates the first Setup before timing, so the stock mesh is drawn with the toolpaths.
- `FLUUT_NO_VSYNC=1` presents with `AutoNoVsync` (immediate or mailbox), so the interval is the cost of a frame and not the display's refresh interval.
- Startup: `FLUUT_STARTUP=1`, six runs.
- Renderer alone: `cargo run --release -p fluut-app --example viewport_bench -- <project> <segments>`: 1280 × 800, waits for the GPU after every frame, so it includes GPU time. Synthetic zig-zag toolpath of 0, 1 M and 2 M segments over the model.

## Results

### Orbit, whole window, vsync off (p50 / p95 ms, range of three runs)

| Project | frame p50 | frame p95 | cpu p50 | with simulated stock: frame p50 / p95 |
|---|---|---|---|---|
| empty window | 0.58–0.59 | 0.59–0.95 | 0.34–0.35 | — |
| `block` | 1.11–1.13 | 1.19–1.29 | 0.87–0.89 | 1.45 / 1.76 |
| `boss` | 1.10 | 1.14–1.21 | 0.85–0.86 | — |
| `ridge` | 1.09 | 1.17–1.21 | 0.84–0.85 | 1.39 / 2.05 |
| `plate` (DXF) | 1.09–1.33 | 1.17–2.05 | 0.85–1.01 | — |
| `dome` | 1.11–1.12 | 1.23–1.27 | 0.87–0.88 | 2.17 / 2.75 |
| `rough3d` | 1.10–1.11 | 1.18–1.29 | 0.86 | 1.23 / 1.41 (300 frames, three runs) |
| `flipside` | 1.25–1.38 | 1.38–1.59 | 1.01–1.13 | — |
| `shaft` | 1.13–1.21 | 1.31–2.25 | 0.88–0.93 | — |
| `vcarve` | 1.12–1.21 | 1.29–1.44 | 0.88–0.94 | — |

The worst p99 over all runs was 2.6 ms and the worst single frame 3.3 ms, apart from one simulated `rough3d` run (p50 3.7, p99 11.7, max 21.7 ms) that three repeats did not reproduce (p50 1.23, max 1.83): another process on the shared machine.

### Renderer alone, 1280 × 800, GPU waited for (p50 / p95 ms)

| Project (triangles) | no toolpath | 1 M segments | 2 M segments |
|---|---|---|---|
| `block` (31 012) | 0.4 / 0.5 | 0.8 / 1.2 | 1.0 / 1.2 |
| `dome` (13 834) | 0.4 / 0.5 | 0.7 / 1.1 | 1.0 / 1.2 |
| `rough3d` (8 382) | 0.4 / 0.4 | 0.7 / 1.1 | 1.0 / 1.1 |

| Measure | 1 M segments | 2 M segments |
|---|---|---|
| Build lines (worker thread) | 28 ms | 55 ms |
| Upload buffers | 21 ms | 40–48 ms |
| Pick (9 × 9 px id pass, blocking read-back) | 1.0 ms | 1.2–1.3 ms |

On lavapipe the same renderer took 302 / 395 ms (2 M segments), upload 109 ms and pick 94 ms.

### Startup to the second frame (`FLUUT_STARTUP=1`, vsync on)

| Case | Runs (ms) |
|---|---|
| empty | 502, 507, 513, 513, 520, 520 |
| `block` | 510, 512, 583, 799, 1593, 1604 |
| `rough3d` | 509, 515, 558, 577, 614, 680 |

About 0.5 s empty. Two of six `block` runs took 1.6 s: the machine was shared, but the cause was not found. All are under the 2 s target.

### vsync

With vsync on, every case, the empty window included, ran at 31.7–32.0 ms a frame (p50) with p95 33 ms and now and then 45–60 ms, while the CPU time was 1–3 ms. That is two refresh intervals at 60 Hz, regardless of the content, so it is the display path and not the app: the session is a disconnected remote-desktop session with the virtual `WinDisc` display. The vsync-on numbers therefore say nothing about frame pacing. Frame pacing at the display rate on a real monitor remains to be checked.

The vsync-off numbers are the cost of rendering and presenting a frame as far as the CPU side can tell (the GPU may run a frame behind). `viewport_bench` waits for the GPU each frame and agrees with them (1 ms at 2 M segments).

## Findings

- No hotspot. Every frame, including the simulated stock and 2 M synthetic segments, is about 1 ms, 15 times under the 16.7 ms budget. Orbiting does not rebuild any buffer; the cost is the egui tessellation and the paint callback.
- The D13 §5.4 targets for frame time, upload (the build is on the worker; the buffer upload is on the UI thread and takes 21 ms for 1 M segments, 40–48 ms for 2 M, inside the 200 ms) and pick latency (1.3 ms) are met on this GPU.
- Pick cost on a real GPU is 1 ms where lavapipe showed 94 ms, so the "culling before the pick pass" deviation in the M5 plan is not urgent on desktop GPUs.
- Not measured: a 50 k-face model (the reference models have 8–31 k triangles, and the repo has no larger STEP or STL fixture), an integrated or mid-range GPU, macOS and Linux, a real monitor's frame pacing, GPU time apart from the CPU's view (no timestamp queries).
- No user-visible requirement follows, so `install.md` and the wiki are unchanged: the program ran at 1 ms a frame on a 2019 workstation card.

## Harness changes

- The header line `bench adapter="…" backend=… type=… driver="…" format=… window=WxH vsync=on|off simulation=true|false`.
- `p99_ms` in the summary, and a second `cpu frames=…` line (eframe's CPU time of each timed frame).
- `FLUUT_BENCH_SIM=1` simulates the first Setup before timing.
- `FLUUT_NO_VSYNC=1` selects `PresentMode::AutoNoVsync`.
- Tests: the p99 in `summary`, and a frames-mode test for the warm-up and the exit.
- The project is the first argument as before (no `FLUUT_BENCH_PROJECT` was needed).

- [x] **Step 1:** Harness; build; measure; write this plan, the M5 plan section, D13 §5.4 and the README row.
- [x] **Step 2:** Checks; commit.
