# STL Bodies Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** An imported STL becomes bodies that the viewport shows, the browser lists, picking selects, Setups use (stock bounds, body stock) and the 3-D toolpaths machine. Today the import fails with `model.import.empty` because meshes give no bodies.

**Architecture:** `fluut-io-mesh` already reads, welds and checks STL. `fluut_topo::mesh::to_breps` turns the welded mesh into one B-rep body per connected shell, each triangle a planar face with line edges shared between neighbours. Everything that takes a `BrepBody` (cutter queries, sections, simulation, scene, picking, re-import) then works on meshes unchanged; no new primitive is needed because D07's drop already solves planes, lines and vertices exactly for flat, ball and bull cutters.

**Design:** [D05](../design/D05-mesh-2d-import.md) §2.5 (new), [D03](../design/D03-topology-bvh.md) §1, [D07](../design/D07-cutter-queries.md); spec 03 §3.2.5.

## Global Constraints

All earlier Global Constraints apply. In addition:

- Imported geometry is untrusted: bad meshes give diagnostics and never panic.
- Golden G-code must not change; section changes only act where a first try left an open chain.
- STL has no unit: the current rule stays (the project unit, with `model.import.unit_assumed`). `import_model_unit` already takes another unit; an inspector control for it is not part of this plan.

## Decisions

1. **Representation.** Triangle faces in `BrepBody` (flag `from_mesh`), not a second body kind. Costs about 1 KB per triangle; accepted for hobby meshes (up to a few hundred thousand triangles). A compact primitive is a follow-up.
2. **Tolerance.** Mesh bodies carry `tol_model` 1e-6 mm. Section chains join within 1000 times it; a weld tolerance of 1e-4 would join over 0.1 mm.
3. **Sections near flat levels.** A plane a hair above a mesh vertex leaves pieces shorter than the join tolerance whose ends meet the same neighbours. When the first chaining leaves an open chain, it is retried without pieces shorter than half the tolerance, then with a hundredth of the tolerance.
4. **Ids.** Bodies `b-1…` per shell; faces `f-<n>.<i>` by triangle order in the shell. Same file, same ids.
5. **Viewport.** Only creases (over 30°) and borders are drawn as edges.
6. **Open meshes.** Warning `model.geometry.mesh.open`; the import succeeds if any triangle remains.

## Tasks

### Task 1: Plan
- [x] Plan committed.

### Task 2: Bodies from meshes
- [x] Tests first: store loads an STL as bodies; two shells are two bodies; a lone triangle and junk files give diagnostics.
- [x] `to_breps`, `BrepBody::from_mesh`, `load_stl`, import message.

### Task 3: Queries and toolpaths
- [x] Tests first: drops of a meshed `dome` equal the exact part within 0.02 mm for ball, flat and bull tools; sections close beside flat levels.
- [x] Section chain retry.
- [x] End to end: import, Setup, `optimised_load_3d` and `parallel`, Calculate, simulation without gouge, Post; re-import keeps ids.

### Task 4: Viewport
- [x] Test first: a mesh scene draws creases and borders only, keeps pick indices.
- [x] `sharp_edges` in the scene.

### Task 5: Docs and checks
- [x] D05 §2.5.
- [x] Format, clippy, tests, layers, deny, wiki reference.
