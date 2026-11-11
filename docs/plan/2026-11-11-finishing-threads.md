# Finishing Threads Plan (fu-9)

**Goal:** `parallel`, `spiral` and `scallop` 3-D finishing ran on one thread (scaling 1.0 on 12 threads in the [drop-speed-2](2026-11-11-drop-speed-2.md) benchmark) while 91-99 % of their time is drops. Run them on all threads with byte-identical output for any thread count.

**Architecture:** all in `fluut-cam`, no change to the drop query.

- *Adaptive sampler by levels (`follow::Sampler::samples_on`).* The sampler bisects chords; whether a chord `(a, b)` is split depends only on its two end samples and the drop at its middle, never on its neighbours. So the bisection tree is walked level by level: every open chord of a level is decided (its middle sampled) in parallel, the children are built in left-to-right order, and the output is the first sample and then the end sample of each accepted chord, left to right, as the depth-first walk gave. The same drops are taken (drop counts of the benchmark are identical before and after) and the points are the same. Levels with fewer than 8 open chords run on the calling thread. The cut-off bisection (a chain of 24 chords per cut/uncut change) advances in step for all changes at once.
- *Pieces of a drive* between corners (`Sampler::samples`, loops of lines and arcs) are sampled in parallel and joined in order.
- *`parallel`:* the passes are laid out in order (zig-zag parity, pass numbers), then sampled in parallel and concatenated in pass order.
- *`spiral`:* the clipped stretches of the spiral and the `machine_corners` loops are sampled in parallel, runs pushed in stretch order. The clip itself (region tests, no drops) stays serial.
- *`scallop`:* the loops of one pass are sampled in parallel; the passes stay in sequence (each step depends on the steepest contact of the next pass). The level-by-level sampler gives the parallelism along a loop.
- *Linker (`Linker::link`):* the stay-down links (`follow`) and the lead-in and lead-out and their gouge checks depend on the runs only: computed in parallel for all runs, used in order. `level_clear` is now `Sync`; `Drive` is `Sync`.

No shared mutable state, no `HashMap`, no atomics in results; every parallel stage collects into a `Vec` in index order. There is no cancellation or progress reporting in the strategies (waterline has none either), so nothing to keep.

## Determinism

`crates/fluut-cam/tests/finishing_threads.rs` runs each type, with and without leads, stay-down links, `machine_corners`, zig-zag at 30 degrees, inside-out `continuous` scallop, on a free-form model (`synth::sculpted`, ball) and an analytic one (the dome, ball and bull), on local `rayon` pools of 1, 3 and 8 threads, and compares `Debug` of the result (every `f64` round-trips) and of the diagnostics exactly. Passes. The benchmark's drop counts and CL point counts are identical before and after for every case; the existing strategy tests and golden G-code are unchanged.

## Measurements

`finishing_bench` (release, Windows 11, 12 threads = 6 cores with SMT, Ø6 flat / ball / bull r1, stepover 0.5 mm, tolerance 0.01 mm). Time in s as flat / ball / bull; before = one thread (identical to 12 threads before), after = 12 threads; one thread after is within 2 % of before.

| Model | Type | Before | After, 12 threads | Speed-up |
|---|---|---|---|---|
| sculpted 3 mm | parallel | 2.15 / 3.68 / 3.83 | 0.46 / 0.72 / 0.70 | 4.6 / 5.1 / 5.5 |
| | spiral | 3.38 / 5.78 / 6.00 | 0.68 / 1.02 / 0.94 | 5.0 / 5.7 / 6.4 |
| | scallop | 5.73 / 10.2 / 10.3 | 1.20 / 1.82 / 1.92 | 4.8 / 5.6 / 5.4 |
| sculpted 12 mm | parallel | 1.77 / 2.36 / 2.65 | 0.32 / 0.38 / 0.42 | 5.5 / 6.2 / 6.3 |
| | spiral | 2.89 / 3.77 / 4.29 | 0.46 / 0.58 / 0.64 | 6.3 / 6.5 / 6.7 |
| | scallop | 4.79 / 5.72 / 7.22 | 1.00 / 1.09 / 1.41 | 4.8 / 5.2 / 5.1 |
| rough tiles | parallel | 5.4 / 6.9 / 5.8 | 1.11 / 1.39 / 1.13 | 4.9 / 5.0 / 5.1 |
| | spiral | 9.0 / 9.9 / 8.7 | 1.57 / 1.73 / 1.49 | 5.7 / 5.7 / 5.8 |
| | scallop | 36.8 / 38.7 / 66.2 | 6.56 / 6.02 / 11.3 | 5.6 / 6.4 / 5.8 |
| dome | parallel | 0.07 / 0.10 / 0.11 | 0.02 / 0.02 / 0.02 | 4-5 |
| | spiral | 0.08 / 0.08 / 0.08 | 0.04 / 0.04 / 0.04 | 2 |
| | scallop | 0.54 / 0.79 / 0.94 | 0.33 / 0.36 / 0.46 | 1.6 / 2.2 / 2.0 |
| rough3d | parallel | 0.10 / 0.02 / 0.10 | 0.02 / 0.01 / 0.01 | 4-7 |
| | spiral | 0.35 / 0.06 / 0.37 | 0.05 / 0.02 / 0.06 | 4-7 |
| | scallop | 0.14 / 0.03 / 0.14 | 0.03 / 0.01 / 0.03 | 3-5 |

On free-form parts the three types now scale 4.6-6.7 times, about the 6 cores' worth. Where less: the dome's `spiral` and `scallop` (analytic, 1-3 µs drops, so the serial remainder shows: the spiral's clip, region offsets, `finish`, arc fitting, are 25-35 % there), and the first levels of each pass's tree, which hold fewer chords than threads.

## Not done

- The serial remainder (spiral clip, scallop offsets, `finish` arc fitting) is not parallel; only visible on analytic parts.
- Scallop passes are sequential by design.
- The corpus body (`nist_ctc_02`) was not re-run.
