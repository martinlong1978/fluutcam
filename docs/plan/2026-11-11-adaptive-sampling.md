# Adaptive Sampling Plan (fu-8)

**Goal:** fewer drops along 3-D finishing passes at the same toolpath tolerance, with no loss of safety ([drop-speed-2](2026-11-11-drop-speed-2.md) Part 1 estimated 40-55 % fewer drops for a predictive sampler). Every output CL point stays a real drop; the chord check stays at least as strong; the level-by-level parallel walk of [finishing-threads](2026-11-11-finishing-threads.md) stays byte-identical for any thread count.

**Design:** [D08 §4.2](../design/D08-toolpath-framework.md), [D09](../design/D09-strategies.md) §4 (`parallel`, `spiral`, `scallop`), [D07 §8](../design/D07-cutter-queries.md).

**Outcome in one line:** the drop count of `spiral` falls 14-31 % and of `scallop` 13-23 % (1.15-1.34 times faster on one thread); `parallel` keeps its drops; every drop taken (middle checks included) now bounds the merged output; predictive step lengths were built two ways and measured, and lose to the bisection on free-form parts: dropped. The 40-55 % estimate does not hold with one real check drop per chord (see Why).

## The guarantee

**Before** (`follow::Sampler`, base main 23af845). Along a drive from `t0` to `t1` the sampler returns samples, each a real drop (raised to `bottom`) at a point of the drive. Between two consecutive samples `a`, `b` (a *sampler chord*):
1. if `a` and `b` differ in cut state, the chord is at most `CUTOFF` = 1e-7 mm long (cut-off points bisected);
2. if both are uncut, the chord is at most `max_step` (R/2) long;
3. if both are cut, the chord is at most `max_step` long and a real drop `m` at the drive's middle parameter was taken, is cut, and `|m.z − (a.z + b.z)/2| ≤ tol/4` — unless the chord is at most 1e-6 mm long (a vertical step, a flat tool over a wall);
4. on a curved drive (arcs of loops, the spiral) also: the drive's sagitta over the chord is at most `tol/4`, and the drop at the chord's own XY middle `cm` (where the tool passes, off the drive by the sagitta) is not **above** the chord by more than `tol/4` (one-sided). On straight pieces of loops the sagitta is rounding noise (about 1e-16 mm) and this second drop was taken too, next to `m`.

`m` (and the `cm` drop) were then thrown away. `cut_runs` merges the samples greedily: an output chord replaces samples that all lie within `tol/4` (3-D distance) of it. So every *sample* is within `tol/4` of the output, and every thrown-away middle drop was within `tol/4` of its sampler chord (vertically), which may itself be up to `tol/4` from the output: up to about `tol/2` from the output.

**After.** 1 and 2 unchanged. 3 unchanged on line drives (same acceptance, same drops; the bisection tree is the same). 4 on curved drives: the drop at the chord's own middle `cm` is taken always, must be cut like the ends, and within `tol/4` of the chord on **either** side (stronger: was one-sided, cut state not checked); the sagitta test is unchanged. The drop at the drive's middle `m` is taken first, and must then also be within `tol/4`, except for chords whose parent's error predicts them to pass (see Approach): for those `m` is taken only if the chord is split, as its new end. Straight pieces of a loop (sagitta below 1e-12 mm, where `cm` and `m` are the same point to 1e-12 mm) take the one drop at `m`.

And: the middle drop `m` of every accepted chord, when taken, is kept as a *check* sample. The merge (`merge_checked`) keeps the output within `tol/4` of check samples too but never ends an output chord at one. So after the merge **every drop on the drive, ends and middles, is within `tol/4` (3-D) of the output chord that replaces it** (was: only the ends), and no output chord is longer than the merge without the checks would make it. The gouge check of a chord (the drop where the tool passes, at `cm`, on a curved drive; at `m` = `cm` on a line) is the same drop with the same or a stricter test.

What is not checked, before or after: points of a chord other than its middle. A flat or bull tool's CL has creases (the contact jumps between two points of the surface), and a crease near a quarter of an accepted chord can put that quarter point off the chord by more than the tolerance while the middle passes. Measured below: a few quarter points per 100 000 on the smooth free-form models and about 1 % on the rough tiles with a flat tool, before and after alike.

## Approach

All in `crates/fluut-cam/src/follow.rs`: `Sampler::samples_on`, `Sampler::split`, `Sampler::cut_runs_slope` and `Sampler::follow` (which merge with `merge_checked`), `Sample::check`; no change to the drop query, the passes, linking or `merge`.

- *Middle drops kept as checks (kept).* `Node::Done` carries the middle drop; the output is the first sample, then per accepted chord its middle drop (when taken, flagged `check`) and its end, left to right. Same drops; the merge must hold every check, so it keeps 1-2 % more CL points on `parallel`.
- *Curved chords checked at their middle (kept).* As in the guarantee. Prediction only chooses the order: a chord is `Hint::Pass` when its parent was split with error `e ≤ 4 · 0.8 · tol/4` (a smooth CL's error falls as the square of the length, so the halves are expected at `e/4`); such a chord drops `cm` first and, if it passes, never needs `m`. A rejected `Pass` chord costs `cm` and then `m` (its new end); every other chord drops `m` first, as before, and `cm` only when `m` passes. Deterministic: the hint depends only on the parent chord.
- *Straight loop pieces (kept).* `SAG_LINE` = 1e-12 mm: one drop where the chord's and the drive's middles coincide.

**Tried and dropped** (measured with `finishing_bench`; code not kept):

- *Middle drops as output vertices.* Same drops, and the greedy merge, free to end chords at the middles, keeps 15-20 % fewer CL points on `parallel` (ridge golden 1 746 to 1 452 G1 moves). But its longer chords step over creases of flat and bull CLs more often: on the rough tiles 8-10 % more quarter points into the part by more than the tolerance (`parallel` flat 546 to 603, `spiral` flat 388 to 419, `spiral` bull 16 to 21). As checks only: 536, 383, 16. Dropped for the safety measure; fewer CL points is not the goal.

Predicted step lengths (sculpted 3 mm and dome, `parallel`, flat and ball):

| Variant | Drops vs bisection | Quarter-point deviation |
|---|---|---|
| Predictive n-ary split: a rejected chord split into `⌈√(e / (F·tol/4))⌉` equal chords (2-8), halving afterwards; F 0.6 / 0.8 / 1.0 | sculpted +4 to +10 % (ball 59 723 to 65 562 / 61 025 / 59 776); dome −6 % | worse: 0.0149-0.0180 mm into the part (0.0081-0.0096 before) |
| same, initial chords 0.5 or 0.75 `max_step` | +9 to +16 % | as before |
| Predictive march inside each `max_step` chord: step from the last chord's error (`h·√(F·tol/4 / e)`, growth ×2 to ×64), one check drop at each step's middle, known drops reused as ends | +9 to +15 % sculpted; dome +11 to +30 % | worse (0.0149-0.018) |
| Predictive march along the whole pass (no `max_step` pieces, step capped at `max_step`) | +9 to +14 % sculpted; dome −1.5 % at best (growth ×8), +8 to +25 % otherwise | 0.0065-0.0122 |

*Coarser initial spacing* is not open: `max_step` (R/2) is part of the guarantee (features narrower than the spacing are not jumped over, D08 §4.2); the bisection already starts at the whole pass and reaches `max_step` without drops.

**Why the estimate does not hold.** The 40-55 % came from a greedy chord merge over all the drops a pass received, after the fact, which keeps no check drop per chord. With one real middle drop per accepted chord, a chord costs 2 drops (its end and its middle) whatever the sampler; the bisection's leaves fall between half the longest acceptable chord `h*` and `h*`, and a rejected chord's middle becomes an end, so it wastes nothing. Accepted chords on sculpted 3 mm, ball (lengths as `max_step / 2^k`): 1 114 at k = 0, 7 425 at k = 1, 15 327 at k = 2, 5 299 at k = 3; rejected 8 166, 9 051, 2 774, 249. A perfect predictor at 0.9 `h*` would save at most about 25 %; on free-form parts the CL's curvature changes within a chord, the predicted lengths miss both ways, and every miss costs a check drop (an odd split wastes the middle; a march recovers slowly after a wall, from 1e-6 mm chords back to `max_step`). On curved drives the base took two check drops per accepted chord (`m` and `cm`); one of them is what this plan saves.

## Determinism

The tree is still walked level by level; whether and how a chord is split depends on its two ends, its drops and its parent's hint, never on its neighbours or the thread count. `tests/finishing_threads.rs` (1, 3, 8 threads, every type, free-form and analytic) passes unchanged. No `HashMap`, `libm` only.

## Tests

`crates/fluut-cam/tests/chord_tolerance.rs` (new): on `synth::sculpted` (free-form), the dome and the ridge (analytic), with ball, flat and bull tools, for `parallel` (0° and 30°), `spiral` and `scallop` at tolerance 0.01 mm, and on the settings of the golden `dome` (`spiral`, `scallop` continuous, `steep_and_shallow`) and `ridge` (`parallel`) projects at 0.02 mm:
- every end of a cut segment is a real drop (to 1e-9 mm), except waterline points in `steep_and_shallow` and the start and early end of a `scallop` loop (put on an accepted chord by the loop ordering, unchanged by this plan; they must be within the tolerance of the drop along the normal);
- every cut chord at least the tolerance long in XY is within the tolerance (along the contact normal) of fresh drops at its 1/4, 1/2 and 3/4 points (level chords along a wall: sideways, as in `assert_no_gouge`);
- `assert_no_gouge` (the strategy tests' dense check) on every result.

The test passes on the base sampler too. The goldens were re-blessed once the test passed on the first version (middle drops as vertices), and re-blessed again for the final version, whose test run (passing) followed the bless and preceded the commit.

## Measurements

`finishing_bench` (feature `bench-stats`), release, Windows 11, 12 threads, Ø6 flat / ball / bull r1, stepover 0.5 mm, tolerance 0.01 mm; base = main 23af845 with the same bench. Drops (one thread), CL points out, time in s on 1 and 12 threads, and the worst chord deviation from fresh drops at 1/4, 1/2, 3/4 of every cut chord at least 0.01 mm long in XY, along the contact normal ("in": into the part, "above": material left). Before → after.

| Model | Type | Tool | Drops | CL points | 1 thread s | 12 threads s | Worst in / above, mm | Points off > tol (into) |
|---|---|---|---|---|---|---|---|---|
| sculpted 3 mm | parallel | flat | 55 636 → 55 636 (+0 %) | 21 626 → 21 947 | 2.16 → 2.15 | 0.49 → 0.47 | 0.0080 / 0.0143 → 0.0080 / 0.0143 | 2 (0) → 2 (0) |
|  | parallel | ball | 59 723 → 59 723 (+0 %) | 25 226 → 25 591 | 3.70 → 3.78 | 0.68 → 0.67 | 0.0096 / 0.0043 → 0.0096 / 0.0043 | 0 (0) → 0 (0) |
|  | parallel | bull | 57 435 → 57 435 (+0 %) | 22 890 → 23 249 | 3.83 → 3.81 | 0.71 → 0.69 | 0.0096 / 0.0161 → 0.0096 / 0.0161 | 1 (0) → 1 (0) |
|  | spiral | flat | 95 119 → 71 139 (-25 %) | 28 678 → 28 255 | 3.41 → 2.66 | 0.61 → 0.50 | 0.0038 / 0.0069 → 0.0038 / 0.0069 | 0 (0) → 0 (0) |
|  | spiral | ball | 96 905 → 72 676 (-25 %) | 30 637 → 30 338 | 5.79 → 4.45 | 0.89 → 0.74 | 0.0040 / 0.0052 → 0.0040 / 0.0052 | 0 (0) → 0 (0) |
|  | spiral | bull | 95 214 → 71 202 (-25 %) | 29 108 → 28 735 | 5.98 → 4.65 | 0.93 → 0.79 | 0.0050 / 0.0073 → 0.0050 / 0.0073 | 0 (0) → 0 (0) |
|  | scallop | flat | 160 455 → 129 533 (-19 %) | 30 809 → 31 368 | 5.94 → 4.70 | 1.23 → 0.98 | 0.0100 / 0.0093 → 0.0064 / 0.0101 | 0 (0) → 1 (0) |
|  | scallop | ball | 166 152 → 138 318 (-17 %) | 34 776 → 35 299 | 10.19 → 8.59 | 1.78 → 1.49 | 0.0045 / 0.0041 → 0.0045 / 0.0039 | 0 (0) → 0 (0) |
|  | scallop | bull | 160 285 → 133 164 (-17 %) | 31 895 → 32 463 | 10.19 → 8.61 | 1.89 → 1.63 | 0.0093 / 0.0180 → 0.0097 / 0.0174 | 2 (0) → 1 (0) |
| sculpted 12 mm | parallel | flat | 53 325 → 53 325 (+0 %) | 21 771 → 21 960 | 1.79 → 1.78 | 0.31 → 0.29 | 0.0077 / 0.0228 → 0.0077 / 0.0228 | 1 (0) → 1 (0) |
|  | parallel | ball | 56 739 → 56 739 (+0 %) | 25 123 → 25 263 | 2.38 → 2.37 | 0.37 → 0.39 | 0.0094 / 0.0061 → 0.0094 / 0.0061 | 0 (0) → 0 (0) |
|  | parallel | bull | 54 883 → 54 883 (+0 %) | 23 059 → 23 239 | 2.65 → 2.63 | 0.43 → 0.42 | 0.0076 / 0.0086 → 0.0076 / 0.0086 | 0 (0) → 0 (0) |
|  | spiral | flat | 91 912 → 68 323 (-26 %) | 28 386 → 28 024 | 2.90 → 2.24 | 0.49 → 0.38 | 0.0037 / 0.0053 → 0.0083 / 0.0053 | 0 (0) → 0 (0) |
|  | spiral | ball | 93 587 → 69 147 (-26 %) | 30 188 → 29 844 | 3.89 → 2.91 | 0.58 → 0.46 | 0.0040 / 0.0047 → 0.0040 / 0.0047 | 0 (0) → 0 (0) |
|  | spiral | bull | 92 347 → 68 559 (-26 %) | 28 811 → 28 492 | 4.30 → 3.31 | 0.64 → 0.54 | 0.0036 / 0.0089 → 0.0036 / 0.0089 | 0 (0) → 0 (0) |
|  | scallop | flat | 147 258 → 115 666 (-21 %) | 29 935 → 30 248 | 4.79 → 3.80 | 0.99 → 0.80 | 0.0132 / 0.0131 → 0.0114 / 0.0135 | 4 (1) → 4 (1) |
|  | scallop | ball | 142 416 → 118 392 (-17 %) | 34 102 → 34 388 | 5.79 → 4.87 | 1.07 → 0.90 | 0.0044 / 0.0051 → 0.0046 / 0.0050 | 0 (0) → 0 (0) |
|  | scallop | bull | 154 179 → 128 061 (-17 %) | 31 407 → 31 752 | 7.23 → 6.06 | 1.41 → 1.31 | 0.0059 / 0.0179 → 0.0055 / 0.0132 | 1 (0) → 1 (0) |
| rough tiles | parallel | flat | 157 185 → 157 185 (+0 %) | 24 268 → 24 676 | 5.21 → 5.23 | 1.10 → 1.09 | 0.1038 / 0.1038 → 0.1038 / 0.1038 | 687 (546) → 678 (536) |
|  | parallel | ball | 185 039 → 185 039 (+0 %) | 77 717 → 78 387 | 6.88 → 6.86 | 1.30 → 1.31 | 0.0238 / 0.0672 → 0.0238 / 0.0672 | 404 (120) → 404 (120) |
|  | parallel | bull | 136 155 → 136 155 (+0 %) | 45 927 → 46 613 | 5.59 → 5.66 | 1.12 → 1.13 | 0.0696 / 0.2080 → 0.0696 / 0.2080 | 304 (90) → 304 (90) |
|  | spiral | flat | 283 122 → 242 696 (-14 %) | 46 024 → 44 977 | 8.78 → 7.66 | 1.52 → 1.39 | 0.0626 / 0.0935 → 0.2081 / 0.2081 | 499 (388) → 495 (383) |
|  | spiral | ball | 275 391 → 229 430 (-17 %) | 75 513 → 76 011 | 9.94 → 8.52 | 1.65 → 1.46 | 0.0123 / 0.0272 → 0.0121 / 0.0272 | 91 (1) → 92 (1) |
|  | spiral | bull | 221 173 → 180 998 (-18 %) | 56 261 → 56 667 | 8.57 → 7.31 | 1.46 → 1.31 | 0.0291 / 0.2102 → 0.0291 / 0.2102 | 138 (16) → 137 (16) |
|  | scallop | flat | 1 200 661 → 1 000 606 (-17 %) | 76 716 → 78 652 | 35.84 → 30.49 | 6.59 → 5.61 | 0.1602 / 0.1623 → 0.1302 / 0.2541 | 1787 (1375) → 1859 (1404) |
|  | scallop | ball | 1 109 347 → 856 124 (-23 %) | 195 471 → 199 161 | 39.08 → 30.32 | 6.21 → 4.80 | 0.0333 / 0.0975 → 0.0259 / 0.0726 | 845 (211) → 762 (208) |
|  | scallop | bull | 1 688 197 → 1 422 907 (-16 %) | 414 781 → 427 028 | 63.86 → 54.72 | 10.74 → 9.40 | 0.0851 / 0.3962 → 0.0806 / 0.2661 | 2827 (725) → 2818 (730) |
| dome | parallel | flat | 37 021 → 37 021 (+0 %) | 6 141 → 6 153 | 0.07 → 0.07 | 0.02 → 0.02 | 0.0027 / 0.0000 → 0.0027 / 0.0000 | 0 (0) → 0 (0) |
|  | parallel | ball | 38 357 → 38 357 (+0 %) | 6 456 → 6 484 | 0.10 → 0.10 | 0.02 → 0.02 | 0.0027 / 0.0000 → 0.0028 / 0.0000 | 0 (0) → 0 (0) |
|  | parallel | bull | 37 513 → 37 513 (+0 %) | 6 289 → 6 301 | 0.11 → 0.12 | 0.02 → 0.02 | 0.0027 / 0.0000 → 0.0026 / 0.0000 | 0 (0) → 0 (0) |
|  | spiral | flat | 62 386 → 42 910 (-31 %) | 19 935 → 19 935 | 0.10 → 0.08 | 0.04 → 0.03 | 0.0024 / 0.0000 → 0.0024 / 0.0000 | 0 (0) → 0 (0) |
|  | spiral | ball | 62 834 → 43 254 (-31 %) | 19 904 → 19 904 | 0.09 → 0.07 | 0.04 → 0.03 | 0.0024 / 0.0000 → 0.0024 / 0.0000 | 0 (0) → 0 (0) |
|  | spiral | bull | 62 538 → 43 026 (-31 %) | 19 924 → 19 924 | 0.10 → 0.08 | 0.04 → 0.03 | 0.0024 / 0.0000 → 0.0024 / 0.0000 | 0 (0) → 0 (0) |
|  | scallop | flat | 261 380 → 226 955 (-13 %) | 47 346 → 47 584 | 0.58 → 0.53 | 0.32 → 0.29 | 0.0027 / 0.0000 → 0.0027 / 0.0000 | 0 (0) → 0 (0) |
|  | scallop | ball | 274 414 → 238 213 (-13 %) | 49 506 → 49 758 | 0.84 → 0.75 | 0.35 → 0.33 | 0.0027 / 0.0000 → 0.0027 / 0.0000 | 0 (0) → 0 (0) |
|  | scallop | bull | 265 561 → 230 535 (-13 %) | 48 082 → 48 302 | 0.98 → 0.87 | 0.49 → 0.41 | 0.0027 / 0.0000 → 0.0027 / 0.0000 | 0 (0) → 0 (0) |
| rough3d | parallel | flat | 10 689 → 10 689 (+0 %) | 81 → 81 | 0.10 → 0.11 | 0.02 → 0.02 | 0.0000 / 0.0000 → 0.0000 / 0.0000 | 0 (0) → 0 (0) |
|  | parallel | ball | 10 689 → 10 689 (+0 %) | 81 → 81 | 0.02 → 0.02 | 0.00 → 0.00 | 0.0000 / 0.0000 → 0.0000 / 0.0000 | 0 (0) → 0 (0) |
|  | parallel | bull | 10 689 → 10 689 (+0 %) | 81 → 81 | 0.10 → 0.10 | 0.02 → 0.02 | 0.0000 / 0.0000 → 0.0000 / 0.0000 | 0 (0) → 0 (0) |
|  | spiral | flat | 35 179 → 26 339 (-25 %) | 11 689 → 11 689 | 0.37 → 0.27 | 0.06 → 0.04 | 0.0018 / 0.0000 → 0.0018 / 0.0000 | 0 (0) → 0 (0) |
|  | spiral | ball | 35 187 → 26 347 (-25 %) | 11 689 → 11 689 | 0.06 → 0.05 | 0.02 → 0.01 | 0.0017 / 0.0000 → 0.0017 / 0.0000 | 0 (0) → 0 (0) |
|  | spiral | bull | 35 183 → 26 343 (-25 %) | 11 689 → 11 689 | 0.33 → 0.25 | 0.05 → 0.04 | 0.0018 / 0.0000 → 0.0018 / 0.0000 | 0 (0) → 0 (0) |
|  | scallop | flat | 14 592 → 12 211 (-16 %) | 2 248 → 2 252 | 0.14 → 0.12 | 0.03 → 0.03 | 0.0032 / 0.0000 → 0.0025 / 0.0000 | 0 (0) → 0 (0) |
|  | scallop | ball | 15 417 → 12 943 (-16 %) | 2 930 → 2 930 | 0.03 → 0.03 | 0.01 → 0.01 | 0.0025 / 0.0000 → 0.0025 / 0.0000 | 0 (0) → 0 (0) |
|  | scallop | bull | 15 051 → 12 411 (-18 %) | 2 468 → 2 474 | 0.14 → 0.12 | 0.03 → 0.03 | 0.0031 / 0.0000 → 0.0025 / 0.0000 | 0 (0) → 0 (0) |

"Points off" counts the quarter, middle and three-quarter points off by more than the tolerance (in brackets: into the part). On the smooth sculpted models 11 before and 11 after (1 into the part each), at creases of the flat or bull CL; none with the ball or on the analytic parts. On the rough tiles (512 faceted faces) about 0.5-1 % of the points, before and after alike: 3 472 into the part before, 3 488 after over all nine cases; the most (`scallop` flat) 1 375 of 183 516 points before, 1 404 of 186 058 after (0.75 % both). The worst single point moves both ways on the tiles (`spiral` flat 0.063 to 0.208 mm, `scallop` flat 0.160 to 0.130 mm into the part): a flat tool climbing onto a plateau, a crease between two samples that the middle check cannot see; the merge then makes a different chord across it.

**Speed.** `spiral` 1.28-1.34 times faster on one thread on the sculpted models and 1.15-1.17 on the rough tiles (12 threads: 1.18-1.29 and 1.09-1.13), `scallop` 1.18-1.29 on one thread (12 threads: 1.08-1.29); on the analytic parts the drops are cheap and the gain is 1.1-1.4 on one thread. `parallel` unchanged within noise.

`contour` uses the sampler only for its stay-down links and `steep_and_shallow` for its shallow part (a `parallel`, line drives): same drops, CL points the same or up to 1 % more (sculpted 3 mm and dome):

| Model | Type | Tool | Drops | CL points | 1 thread s | 12 threads s |
|---|---|---|---|---|---|---|
| sculpted 3 mm | contour | flat | 432 328 → 432 328 (+0 %) | 22 570 → 22 570 | 11.97 → 11.96 | 2.01 → 1.96 |
|  | contour | ball | 395 630 → 395 630 (+0 %) | 26 464 → 26 464 | 16.45 → 16.39 | 2.58 → 2.60 |
|  | contour | bull | 420 665 → 420 665 (+0 %) | 24 251 → 24 251 | 20.39 → 20.21 | 3.05 → 3.07 |
|  | steep_and_shallow | flat | 742 627 → 742 627 (+0 %) | 30 727 → 31 041 | 23.93 → 23.92 | 10.40 → 10.13 |
|  | steep_and_shallow | ball | 563 532 → 563 532 (+0 %) | 33 027 → 33 358 | 26.34 → 26.22 | 5.87 → 5.87 |
|  | steep_and_shallow | bull | 586 364 → 586 364 (+0 %) | 30 050 → 30 394 | 30.17 → 30.24 | 6.56 → 6.61 |
| dome | contour | flat | 153 807 → 153 807 (+0 %) | 10 134 → 10 134 | 0.42 → 0.42 | 0.08 → 0.07 |
|  | contour | ball | 112 343 → 112 343 (+0 %) | 9 660 → 9 660 | 0.65 → 0.65 | 0.10 → 0.10 |
|  | contour | bull | 113 377 → 113 377 (+0 %) | 10 032 → 10 032 | 0.66 → 0.66 | 0.10 → 0.10 |
|  | steep_and_shallow | flat | 329 164 → 329 164 (+0 %) | 12 831 → 12 855 | 0.84 → 0.84 | 0.39 → 0.39 |
|  | steep_and_shallow | ball | 177 520 → 177 520 (+0 %) | 13 522 → 13 546 | 0.80 → 0.82 | 0.15 → 0.15 |
|  | steep_and_shallow | bull | 177 862 → 177 862 (+0 %) | 14 233 → 14 253 | 0.86 → 0.85 | 0.14 → 0.15 |

## Golden G-code

Re-blessed: two files, both from toolpaths that use the sampler; the other eleven are unchanged (the dome's `contour` and `horizontal` sections are unchanged too).

- `crates/fluut-cli/tests/golden/ridge-linuxcnc.ngc`: "Parallel finish" (`parallel`, tolerance 0.02 mm, 45 degrees, slopes up to 25 degrees) gains 8 points (1 196 to 1 204 lines, no line moved): at 8 places the merged chord would have left a middle check drop by more than `tol/4`, so the merge ends a chord one sample earlier. Same drops. "Parallel slopes" is unchanged.
- `crates/fluut-cli/tests/golden/dome-linuxcnc.ngc`: only the shallow `parallel` (45 degrees) of "Steep and shallow": 22 points moved, same line count, where the passes run down the steep side of the dome to the threshold, for the same reason (the merge holds the middle checks). The `spiral` and `scallop` sections are byte-identical although they take about 30 % fewer drops (the chord ends are the same; the drops saved are the second check of each curved chord); waterline and flats unchanged.

Every cut point of these toolpaths' settings is a real drop and every chord holds the tolerance at its quarter points (`golden_project_settings_hold_the_tolerance`, on the same parts: the projects pick faces by id, the test drives the whole part).

## Not done

- Points of an accepted chord other than its middle are not checked (before or after); creases of flat and bull CLs can put a quarter point of a chord off by slightly more than the tolerance (11-12 in about 900 000 test points on the sculpted models).
- `scallop` loop starts and continuous loop ends are interpolated on an accepted chord, not dropped (unchanged).
- `nist_ctc_02` was not re-run.
