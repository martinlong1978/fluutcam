# Crease Checks Plan (fu-10)

**Goal:** the adaptive sampler ([adaptive sampling](2026-11-11-adaptive-sampling.md), fu-8) checks every accepted chord with one real drop at its middle. The CL of a flat or bull tool has creases (the contact jumps from one face, edge or vertex to another, or from the rim to the flat bottom), and two creases inside one chord can cancel at its middle. Before this plan the quarter points of output chords were off by more than the tolerance at 11 points on the sculpted models and at 0.5-1 % of the points on the rough tiles, the worst 0.21 mm into the part (flat tool, `spiral`). Remove or bound those misses at the smallest extra drop cost; every output point stays a real drop; the output stays the same for any thread count.

**Design:** [D08 §4.2](../design/D08-toolpath-framework.md), [D09](../design/D09-strategies.md) §4, [D07 §8](../design/D07-cutter-queries.md).

**Outcome in one line:** each drop also reports the CL's gradient (from its contact; heights unchanged, no extra solver work); a cut chord is split when the gradients at its ends and middle bound the CL's distance from it above `tol/2`. No quarter point is off by more than the tolerance on the sculpted and analytic models any more (11 before), none with the ball and 1 into the part (by 0.0163 mm along the normal, cut depth within the tolerance) with the bull tool on the rough tiles (836 into the part before), and with the flat tool on the tiles the worst cut depth falls from 0.08-0.16 mm to 0.010-0.020 mm, at 4 points out of about 320 000, each one a drop that misses a contact (not a crease the sampler steps over). Cost: 0-3 % more drops on the free-form models, none on the analytic ones, 5-11 % on the rough tiles.

## The check

All in `crates/fluut-cam/src/follow.rs` (`Sampler::split`, new `Sampler::crease`) and `surface.rs` (`Hit::grad`, `SurfaceQuery::slope_grad`); the drop's heights, slopes and the bisection's other tests are unchanged.

**The gradient (`Hit::grad`).** The tip height `τ(x, y)` of a drop is the highest position of the tool over the geometry, so where the contact is unique and smooth its gradient follows from the contact alone (the envelope theorem: differentiate the contact constraint by the axis position): `τ` rises towards the contact (`toward`, the XY direction from the axis to the contact point) at the rate
- ball and bull tools, a contact on the round of the tool: `tan β`, `β` the slope of the tool's surface there (as `Hit::slope`);
- flat tool, a contact on the rim against a face: `tan β`, `β` the face's slope (the rim meets the face where the face rises along `toward`);
- flat tool, the rim against an edge: `|z'| / |toward·xy'|` with `(xy', z')` the edge's tangent (the rim meets the edge where it rises as fast as it leaves the axis: the Lagrange multiplier of the constraint `|edge − axis| = R`). This is not `tan` of the edge's own rise angle, which `Hit::slope` reports there;
- a contact under the flat bottom (a peak, a flat face): 0; a vertex on the rim, an overhang: a wall, `GRAD_MAX` = 1e3.

Each component is capped at `GRAD_MAX`. The cost is a `hypot` and a division per drop (the face normal was already evaluated for the slope). Where the CL has a crease the gradient is the one of the winning contact, that is of one side.

**The bound (`Sampler::crease`).** A cut chord from `a` to `b`, XY length `l`, that has passed the bisection's tests (its middle drop `c` cut and within `tol/4`) is split if it is longer than `CREASE_MIN·tol` (0.25 `tol`) and `E > CREASE_LIM·tol` (0.5 `tol`), where, with `s` the chord's slope, `ga`, `gb` the gradients along the chord, `da = (ga − s)·l`, `db = (s − gb)·l` (the tangents' offsets from the chord across it):
- `off(a, b) = da·db/(da + db)` if `da` and `db` have the same sign, else `max(|da|, |db|)/2`;
- `E = max(off(a, b), off(a, c) + |c − mid|, off(c, b) + |c − mid|)`, the halves' bounds (with `c`'s gradient) plus how far the half chords are from the whole one.

A split chord is split at the drive's middle as any other (its middle drop becomes an end; hint `Unknown`). Every output point is still a real drop, and the decision still depends only on the chord's own drops: the level-by-level walk is unchanged and the result is the same for any thread count.

**Why this bound (and where it is sound).** Let `f` be the CL along the chord minus the chord, so `f(0) = f(l) = 0`, `f'(0) = ga − s`, `f'(l) = gb − s`.
1. *`f` concave or convex between the drops* (any number of creases, all of one kind, with straight or bent pieces between): a concave `f` lies below each of its end tangents and above the chord, so `0 ≤ f ≤ min(da·x/l, db·(l − x)/l)·…`, whose largest value is exactly `da·db/(da + db)` (the tent of the two tangents). Sound and sharp: one crease between straight pieces reaches it. For a parabola with middle error `e`, `da = db = 4e` and the bound is `2e ≤ tol/2`: smooth chords that pass the middle test pass this one, with room.
2. *Two creases of opposite kinds between straight pieces* (the S a flat tool makes climbing onto a plateau: flat, ramp, flat; the case the middle cannot see): `da` and `db` then have opposite signs, and the errors at the creases `p < q` (as fractions of `l`) are `da·p` and `db·(1 − q)`. If `p ≤ 1/2 ≤ q` both are at most `max(|da|, |db|)/2`; if both creases are in one half, the middle lies on the last (or first) piece, its error is `db/2` (or `da/2`), the middle test bounds that by `tol/4`, and the errors are at most `|db|` (`|da|`) `≤ tol/2` and `|da|/2`. Sound for this shape.
3. *Anything else* (three or more creases between two drops, creases of both kinds with curved pieces, an inflection with creases) is not bounded by the end gradients alone. The halves term catches more of it (a bend, a crease and a bend in one chord: found on the tiles with the ball, `scallop`), because it uses `c`'s gradient too. This is a heuristic beyond 1 and 2, as is every check that samples a finite number of points; the measurements below show what is left.
4. *Chords up to `tol/4` in XY* are not checked (they are steps up or down walls: the error is sideways there, within `tol/8` of a real drop).
5. The bound needs the gradients to be right, and the drops to be right. Where a drop misses a contact (the solver's starts do not find one) both the height and the gradient of that sample are wrong, and no check on the samples can see it.

So the guarantee is: every accepted cut chord whose CL is of shape 1 or 2 between its drops lies within `tol/2` of the CL; the merge keeps the output within `tol/4` of every drop; so the output is within `3/4·tol` of the CL at such chords. The tightened test asserts that bound on the test parts (below).

## How the deviation is measured

The adaptive-sampling plan measured a quarter point's deviation as `(τ − z)·cos β` with `β = Hit::slope`. Where a flat tool's rim touches an edge, `β` is the edge's own rise angle, while the CL is much steeper (rim on the foot of a wall: CL gradients of 15-40 against an edge slope of 50-60°): there the measure says 0.01-0.035 mm where the chord is within a few µm of the CL surface. 1 300 of the 1 400 flat-tool "misses into the part" on the tiles (`scallop`) were this. `finishing_bench` now reports three measures for each quarter point:
- *tool normal*: as before, for comparison;
- *CL normal*: `(τ − z)/√(1 + |∇τ|²)`, the distance along the CL surface's normal (the same as the tool normal except at flat-rim edge contacts);
- *cut depth* (into the part only): how deep the tool there cuts into the part, at most. The tool shrunk by `s` all round (radius and corner `s` smaller, tip `s` higher) touches nothing if and only if everything the tool overlaps is within `s` of its surface. Computed for `s` = 1/4, 1/2, 1, 2, 4 ... 32 `tol` with separate queries; the reported depth is the smallest such `s` (or the vertical lift if that is less). Exact up to those steps: the dome's 0.0100 below means "between 0.005 and 0.01" (steep walls, where the vertical lift is more than `tol`).

`FB_WORST=1` also prints each point cut deeper than the tolerance, `FB_WORST=all` every point off.

## Tried and dropped

- *Contact identity as the trigger* (check only chords whose end and middle drops touch different faces, edges or vertices; a `Touch` field in `Hit`). Misses creases on one entity: a flat tool over one B-spline edge of the tiles went from the edge's peak under the bottom to its flank on the rim and back to another peak of the same edge (all three drops on edge 1036, 0.064 mm off at the quarters); and a contact can jump within one face. Checking every cut chord cost only 0.1-0.5 % more drops than the trigger: no trigger, and no `Touch`.
- *End gradients only* (`off(a, b)` without the halves): 0.4-2 % fewer drops, but a ball on the tiles (`scallop`) kept a chord whose CL bends, creases (a valley) and bends again with the end tangents agreeing with the chord: 0.025 mm cut depth. The halves term catches it.
- *`CREASE_LIM` 1.0*: 1.5 % fewer drops on the sculpted model, and misses with ball and bull on the tiles (cut depth 0.011-0.015 mm). *`CREASE_MIN` 0.5 or 0.1*: no measurable difference to 0.25.


## Determinism

The decision for a chord uses its own two ends, its middle drops and their gradients (all real drops), never its neighbours or the thread count; the walk, the merge and the parallel stages are unchanged. `tests/finishing_threads.rs` (1, 3 and 8 threads, every type, free-form and analytic) passes unchanged. No `HashMap`; `libm` (`hypot`, `tan`, `acos`, `asin`) only.

## Tests

`crates/fluut-cam/tests/chord_tolerance.rs`, tightened:
- the quarter, middle and three-quarter points of every cut chord at least `tol` long in XY must be within **`0.75·tol`** (was `tol`) of fresh drops along the CL surface's normal (was the tool's normal at the contact, which is not the CL's at flat-rim edge contacts), or, into the part, the tool there must cut less than `0.75·tol` deep (the shrunk tool touches nothing);
- new part: the rough tiles (`synth::tiled(2, true)`, 32 B-spline bumps at random heights with open edges), ball, flat and bull, `parallel` (0° and 30°), `spiral`, `scallop`. Its dense check is the strategy tests' check with the cut depth in place of the sideways tests (`assert_shallow`: a flat tool's wall steps on the tiles are a few µm wide in XY, and a link down one is not recognised as vertical by `assert_no_gouge`);
- unchanged: every cut point a real drop (the loop-end exceptions of `scallop` and the waterline points of `steep_and_shallow`), the golden-project settings, `assert_no_gouge` on the other parts.

With the crease check switched off (the base sampler) the tiles test fails (`parallel`, ball: a quarter point 0.011 mm off); the sculpted, dome and ridge parts pass on the base too at `0.75·tol` (their misses were on the larger bench models).

## Measurements

`finishing_bench` (feature `bench-stats`), release, Windows 11, 12 threads = 6 cores with SMT, Ø6 flat / ball / bull r1, stepover 0.5 mm, tolerance 0.01 mm, `FB_SKIP=1`. Before = this branch with the crease check off (the base sampler's drops and output, with the new measures; the tool-normal counts are those of the adaptive-sampling plan), after = final. Times in s, one run each (they move by a few % between runs: the dome's `scallop` bull at 1.02 → 0.91 is noise). "Off > tol" counts quarter, middle and three-quarter points off by more than the tolerance, into the part in brackets; "cut deeper than tol" the points whose cut depth (see above) is more than the tolerance.

| Model | Type | Tool | Drops | 1 thread s | 12 threads s | Off > tol, tool normal (into) | CL normal: worst in / above, mm | CL normal: off > tol (into) | Cut deeper than tol: worst mm (points) |
|---|---|---|---|---|---|---|---|---|---|
| sculpted 3 mm | parallel | flat | 55 636 → 57 442 (+3.2 %) | 2.18 → 2.27 | 0.48 → 0.48 | 2 (0) → 0 (0) | 0.0080 / 0.0143 → 0.0040 / 0.0034 | 2 (0) → 0 (0) | 0.0081 (0) → 0.0053 (0) |
|  | parallel | ball | 59 723 → 61 115 (+2.3 %) | 3.79 → 3.91 | 0.68 → 0.69 | 0 (0) → 0 (0) | 0.0096 / 0.0043 → 0.0037 / 0.0033 | 0 (0) → 0 (0) | 0.0100 (0) → 0.0040 (0) |
|  | parallel | bull | 57 435 → 58 905 (+2.6 %) | 3.89 → 3.95 | 0.69 → 0.72 | 1 (0) → 0 (0) | 0.0096 / 0.0161 → 0.0035 / 0.0035 | 1 (0) → 0 (0) | 0.0100 (0) → 0.0040 (0) |
|  | spiral | flat | 71 139 → 72 605 (+2.1 %) | 2.77 → 2.79 | 0.52 → 0.52 | 0 (0) → 0 (0) | 0.0037 / 0.0069 → 0.0035 / 0.0041 | 0 (0) → 0 (0) | 0.0039 (0) → 0.0039 (0) |
|  | spiral | ball | 72 676 → 73 905 (+1.7 %) | 4.55 → 4.55 | 0.74 → 0.74 | 0 (0) → 0 (0) | 0.0040 / 0.0052 → 0.0031 / 0.0036 | 0 (0) → 0 (0) | 0.0050 (0) → 0.0039 (0) |
|  | spiral | bull | 71 202 → 72 706 (+2.1 %) | 4.68 → 4.79 | 0.76 → 0.78 | 0 (0) → 0 (0) | 0.0050 / 0.0073 → 0.0032 / 0.0038 | 0 (0) → 0 (0) | 0.0054 (0) → 0.0039 (0) |
|  | scallop | flat | 129 533 → 132 805 (+2.5 %) | 4.81 → 4.85 | 1.14 → 1.01 | 1 (0) → 0 (0) | 0.0064 / 0.0101 → 0.0034 / 0.0036 | 1 (0) → 0 (0) | 0.0071 (0) → 0.0037 (0) |
|  | scallop | ball | 138 318 → 139 450 (+0.8 %) | 8.58 → 8.63 | 1.58 → 1.51 | 0 (0) → 0 (0) | 0.0045 / 0.0039 → 0.0041 / 0.0035 | 0 (0) → 0 (0) | 0.0055 (0) → 0.0044 (0) |
|  | scallop | bull | 133 164 → 136 230 (+2.3 %) | 8.70 → 8.76 | 1.63 → 1.65 | 1 (0) → 0 (0) | 0.0097 / 0.0174 → 0.0035 / 0.0035 | 1 (0) → 0 (0) | 0.0098 (0) → 0.0042 (0) |
| sculpted 12 mm | parallel | flat | 53 325 → 54 325 (+1.9 %) | 1.82 → 1.81 | 0.29 → 0.30 | 1 (0) → 0 (0) | 0.0077 / 0.0228 → 0.0029 / 0.0033 | 1 (0) → 0 (0) | 0.0084 (0) → 0.0036 (0) |
|  | parallel | ball | 56 739 → 57 449 (+1.3 %) | 2.37 → 2.40 | 0.37 → 0.38 | 0 (0) → 0 (0) | 0.0094 / 0.0061 → 0.0031 / 0.0033 | 0 (0) → 0 (0) | 0.0100 (0) → 0.0037 (0) |
|  | parallel | bull | 54 883 → 55 775 (+1.6 %) | 2.66 → 2.68 | 0.43 → 0.44 | 0 (0) → 0 (0) | 0.0076 / 0.0086 → 0.0031 / 0.0033 | 0 (0) → 0 (0) | 0.0076 (0) → 0.0040 (0) |
|  | spiral | flat | 68 323 → 69 094 (+1.1 %) | 2.32 → 2.29 | 0.39 → 0.37 | 0 (0) → 0 (0) | 0.0075 / 0.0053 → 0.0035 / 0.0037 | 0 (0) → 0 (0) | 0.0084 (0) → 0.0036 (0) |
|  | spiral | ball | 69 147 → 69 602 (+0.7 %) | 2.98 → 2.92 | 0.46 → 0.46 | 0 (0) → 0 (0) | 0.0040 / 0.0047 → 0.0030 / 0.0035 | 0 (0) → 0 (0) | 0.0040 (0) → 0.0037 (0) |
|  | spiral | bull | 68 559 → 69 206 (+0.9 %) | 3.39 → 3.38 | 0.51 → 0.51 | 0 (0) → 0 (0) | 0.0036 / 0.0089 → 0.0033 / 0.0089 | 0 (0) → 0 (0) | 0.0043 (0) → 0.0040 (0) |
|  | scallop | flat | 115 666 → 117 454 (+1.5 %) | 3.86 → 3.87 | 0.79 → 0.80 | 4 (1) → 0 (0) | 0.0114 / 0.0135 → 0.0032 / 0.0037 | 4 (1) → 0 (0) | 0.0100 (0) → 0.0037 (0) |
|  | scallop | ball | 118 392 → 119 458 (+0.9 %) | 4.96 → 4.92 | 0.90 → 0.91 | 0 (0) → 0 (0) | 0.0046 / 0.0050 → 0.0033 / 0.0033 | 0 (0) → 0 (0) | 0.0052 (0) → 0.0037 (0) |
|  | scallop | bull | 128 061 → 128 223 (+0.1 %) | 6.19 → 6.12 | 1.19 → 1.17 | 1 (0) → 0 (0) | 0.0055 / 0.0132 → 0.0054 / 0.0036 | 1 (0) → 0 (0) | 0.0060 (0) → 0.0054 (0) |
| dome | parallel | flat | 37 021 → 37 021 (+0.0 %) | 0.08 → 0.08 | 0.01 → 0.02 | 0 (0) → 0 (0) | 0.0027 / 0.0000 → 0.0027 / 0.0000 | 0 (0) → 0 (0) | 0.0100 (0) → 0.0100 (0) |
|  | parallel | ball | 38 357 → 38 357 (+0.0 %) | 0.10 → 0.10 | 0.02 → 0.02 | 0 (0) → 0 (0) | 0.0028 / 0.0000 → 0.0028 / 0.0000 | 0 (0) → 0 (0) | 0.0100 (0) → 0.0100 (0) |
|  | parallel | bull | 37 513 → 37 521 (+0.0 %) | 0.12 → 0.12 | 0.02 → 0.02 | 0 (0) → 0 (0) | 0.0026 / 0.0000 → 0.0026 / 0.0000 | 0 (0) → 0 (0) | 0.0100 (0) → 0.0100 (0) |
|  | spiral | flat | 42 910 → 42 910 (+0.0 %) | 0.09 → 0.09 | 0.04 → 0.04 | 0 (0) → 0 (0) | 0.0024 / 0.0000 → 0.0024 / 0.0000 | 0 (0) → 0 (0) | 0.0100 (0) → 0.0100 (0) |
|  | spiral | ball | 43 254 → 43 259 (+0.0 %) | 0.09 → 0.09 | 0.04 → 0.05 | 0 (0) → 0 (0) | 0.0024 / 0.0000 → 0.0024 / 0.0000 | 0 (0) → 0 (0) | 0.0100 (0) → 0.0100 (0) |
|  | spiral | bull | 43 026 → 43 026 (+0.0 %) | 0.09 → 0.09 | 0.05 → 0.04 | 0 (0) → 0 (0) | 0.0024 / 0.0000 → 0.0024 / 0.0000 | 0 (0) → 0 (0) | 0.0100 (0) → 0.0100 (0) |
|  | scallop | flat | 226 955 → 226 995 (+0.0 %) | 0.55 → 0.54 | 0.29 → 0.29 | 0 (0) → 0 (0) | 0.0027 / 0.0000 → 0.0027 / 0.0000 | 0 (0) → 0 (0) | 0.0100 (0) → 0.0100 (0) |
|  | scallop | ball | 238 213 → 238 293 (+0.0 %) | 0.78 → 0.79 | 0.35 → 0.35 | 0 (0) → 0 (0) | 0.0027 / 0.0000 → 0.0027 / 0.0000 | 0 (0) → 0 (0) | 0.0100 (0) → 0.0100 (0) |
|  | scallop | bull | 230 535 → 230 583 (+0.0 %) | 1.02 → 0.91 | 0.43 → 0.42 | 0 (0) → 0 (0) | 0.0027 / 0.0000 → 0.0027 / 0.0000 | 0 (0) → 0 (0) | 0.0100 (0) → 0.0100 (0) |
| rough3d | parallel | flat | 10 689 → 10 689 (+0.0 %) | 0.10 → 0.11 | 0.02 → 0.02 | 0 (0) → 0 (0) | 0.0000 / 0.0000 → 0.0000 / 0.0000 | 0 (0) → 0 (0) | 0.0000 (0) → 0.0000 (0) |
|  | parallel | ball | 10 689 → 10 689 (+0.0 %) | 0.02 → 0.02 | 0.00 → 0.00 | 0 (0) → 0 (0) | 0.0000 / 0.0000 → 0.0000 / 0.0000 | 0 (0) → 0 (0) | 0.0000 (0) → 0.0000 (0) |
|  | parallel | bull | 10 689 → 10 689 (+0.0 %) | 0.10 → 0.10 | 0.01 → 0.01 | 0 (0) → 0 (0) | 0.0000 / 0.0000 → 0.0000 / 0.0000 | 0 (0) → 0 (0) | 0.0000 (0) → 0.0000 (0) |
|  | spiral | flat | 26 339 → 26 339 (+0.0 %) | 0.27 → 0.27 | 0.04 → 0.04 | 0 (0) → 0 (0) | 0.0018 / 0.0000 → 0.0018 / 0.0000 | 0 (0) → 0 (0) | 0.0019 (0) → 0.0019 (0) |
|  | spiral | ball | 26 347 → 26 347 (+0.0 %) | 0.05 → 0.05 | 0.01 → 0.02 | 0 (0) → 0 (0) | 0.0017 / 0.0000 → 0.0017 / 0.0000 | 0 (0) → 0 (0) | 0.0019 (0) → 0.0019 (0) |
|  | spiral | bull | 26 343 → 26 343 (+0.0 %) | 0.25 → 0.25 | 0.05 → 0.04 | 0 (0) → 0 (0) | 0.0018 / 0.0000 → 0.0018 / 0.0000 | 0 (0) → 0 (0) | 0.0019 (0) → 0.0019 (0) |
|  | scallop | flat | 12 211 → 12 211 (+0.0 %) | 0.12 → 0.12 | 0.03 → 0.03 | 0 (0) → 0 (0) | 0.0025 / 0.0000 → 0.0025 / 0.0000 | 0 (0) → 0 (0) | 0.0026 (0) → 0.0026 (0) |
|  | scallop | ball | 12 943 → 12 943 (+0.0 %) | 0.03 → 0.03 | 0.01 → 0.01 | 0 (0) → 0 (0) | 0.0025 / 0.0000 → 0.0025 / 0.0000 | 0 (0) → 0 (0) | 0.0026 (0) → 0.0026 (0) |
|  | scallop | bull | 12 411 → 12 411 (+0.0 %) | 0.12 → 0.12 | 0.03 → 0.03 | 0 (0) → 0 (0) | 0.0025 / 0.0000 → 0.0025 / 0.0000 | 0 (0) → 0 (0) | 0.0026 (0) → 0.0026 (0) |
| rough tiles | parallel | flat | 157 185 → 166 317 (+5.8 %) | 5.31 → 5.61 | 1.18 → 1.14 | 678 (536) → 468 (458) | 0.1038 / 0.1038 → 0.0831 / 0.0579 | 221 (92) → 9 (4) | 0.0841 (73) → 0.0100 (0) |
|  | parallel | ball | 185 039 → 200 037 (+8.1 %) | 6.86 → 7.49 | 1.33 → 1.38 | 404 (120) → 0 (0) | 0.0238 / 0.0672 → 0.0028 / 0.0038 | 404 (120) → 0 (0) | 0.0323 (120) → 0.0071 (0) |
|  | parallel | bull | 136 155 → 149 415 (+9.7 %) | 5.60 → 6.19 | 1.14 → 1.25 | 304 (90) → 1 (0) | 0.0696 / 0.2080 → 0.0068 / 0.0147 | 304 (90) → 1 (0) | 0.0800 (90) → 0.0100 (0) |
|  | spiral | flat | 242 696 → 255 507 (+5.3 %) | 7.68 → 8.33 | 1.44 → 1.56 | 495 (383) → 333 (325) | 0.2081 / 0.2081 → 0.0504 / 0.0733 | 159 (58) → 14 (6) | 0.1600 (43) → 0.0200 (3) |
|  | spiral | ball | 229 430 → 250 260 (+9.1 %) | 8.74 → 9.82 | 1.47 → 1.58 | 92 (1) → 0 (0) | 0.0121 / 0.0272 → 0.0038 / 0.0037 | 92 (1) → 0 (0) | 0.0145 (1) → 0.0061 (0) |
|  | spiral | bull | 180 998 → 198 497 (+9.7 %) | 7.26 → 8.06 | 1.34 → 1.46 | 137 (16) → 0 (0) | 0.0291 / 0.2102 → 0.0044 / 0.0044 | 137 (16) → 0 (0) | 0.0400 (16) → 0.0100 (0) |
|  | scallop | flat | 1 000 606 → 1 107 246 (+10.7 %) | 30.61 → 34.20 | 5.65 → 6.16 | 1859 (1404) → 1163 (1136) | 0.1302 / 0.2541 → 0.4487 / 0.1124 | 705 (287) → 30 (7) | 0.1297 (228) → 0.0130 (1) |
|  | scallop | ball | 856 124 → 935 763 (+9.3 %) | 30.38 → 33.88 | 5.51 → 5.33 | 762 (208) → 0 (0) | 0.0259 / 0.0726 → 0.0028 / 0.0039 | 762 (208) → 0 (0) | 0.0315 (207) → 0.0083 (0) |
|  | scallop | bull | 1 422 907 → 1 533 504 (+7.8 %) | 56.18 → 60.24 | 11.01 → 9.94 | 2818 (730) → 10 (1) | 0.0806 / 0.2661 → 0.0163 / 0.1241 | 2818 (730) → 10 (1) | 0.0800 (726) → 0.0100 (0) |

**Free-form and analytic models.** No point off by more than the tolerance by either normal, and none cut deeper (11 points off before, 1 of them into the part). The worst deviation along the CL normal falls from up to 0.0114 mm into and 0.0228 mm above the part to 0.0054 and 0.0089 mm. Drops +0.1 to +3.2 % on the sculpted models, +0.0 to 0.02 % on the dome and `rough3d`; one-thread time +0 to +4 % (in the noise for most), 12 threads within noise.

**Rough tiles.** In the order `parallel` / `spiral` / `scallop`. Ball: no point off (404 / 92 / 762 before, 120 / 1 / 208 into the part). Bull: 1 / 0 / 10 off, of which one into the part (`scallop`, 0.0163 mm along the CL normal, cut depth within the tolerance); 304 / 137 / 2 818 before, 90 / 16 / 730 into. Flat: along the CL normal 9 / 14 / 30 points off (221 / 159 / 705 before), of which 4 / 6 / 7 into the part; cut depth beyond the tolerance at 0 / 3 / 1 points (73 / 43 / 228 before), worst 0.010 / 0.020 / 0.013 mm (0.084 / 0.160 / 0.130 before). Drops +5 to +11 %, one-thread time +5 to +12 %, 12 threads −10 to +10 %. The flat-tool count along the tool normal stays high (458 / 325 / 1 136 into): those are the flat-rim edge contacts where the tool normal is not the CL's (see How the deviation is measured).

**What is left on the tiles.** Every flat-tool point cut deeper than the tolerance (4 in all) was examined: a drop between the chord's samples finds a contact on the tool's rim that lies inside the footprint of one of the chord's own drops (an end or the middle) above that drop's height. That drop missed the contact (the flat-bottom Newton's starts do not find it; drop-speed-2 counted 0-3 such "above the full scan" misses per 3 000 random drops on the tiles). The CL the sampler sees is wrong there, and no check on the samples can see it. Two of them sit where the drops flicker between 2.465 and 2.485 mm along 0.04 mm (a contact exactly on the rim). The flat-tool points off along the CL normal but not cut deeper are chords along a wall (a `scallop` chord of 5.9 mm, nearly level, 0.45 mm under the drop at its middle whose contact is on top of the wall beside it: the tool cuts less than 0.01 mm deep there) and points within the tolerance's steps of the cut-depth measure. So on the tiles the worst depth into the part is within the tolerance for ball and bull; for the flat tool, 4 of 320 000 points (0.010-0.020 mm) remain, from drop misses.

## Golden G-code

None changed: the 13 golden files of `fluut-cli` (ball tools on the dome and ridge, analytic) are byte-identical, so nothing was re-blessed. The extra drops of the dome are 0-0.02 % and fall on chords whose merged output is the same.

## Not done

- Drop misses (the CL is wrong at a sample) are the drop query's ([D07 §8](../design/D07-cutter-queries.md), CH-25), not the sampler's; on the tiles they leave 4 flat-tool points 0.010-0.020 mm deep in 320 000.
- The bound is sound for CLs that bend one way between two drops and for an S of two creases between straight pieces; three or more creases between two drops are a heuristic (none found).
- Chords up to `tol/4` in XY are not checked (wall steps).
- `contour` and `steep_and_shallow` were not re-measured (they use the sampler for links and the shallow part; their drops change as `parallel`'s); `nist_ctc_02` was not re-run.
