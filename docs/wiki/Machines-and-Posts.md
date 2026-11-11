# Machines and posts

Two kinds of definition describe your equipment:

- A **machine** says what your machine can do: travel, spindle speeds, feed limits, a rotary axis, tool changer.
- A **post** (post processor) says how to write G-code for your controller.

FluutCAM ships a few of each, and you can add your own as text files.

## Built-in machines

| Id | What it is |
|---|---|
| `fluut/generic-3axis-router` | A router with 1000 x 1000 mm of X and Y travel and 150 mm of Z, a 6000 to 24000 rev/min spindle, a manual tool changer. A starting point for your own. Its post is `fluut/gcode`. |
| `fluut/generic-4axis-router` | The same, with a rotary axis A along X that can turn without limit. Its post is `fluut/linuxcnc-4x`. |

A new 3-axis Setup uses the first, and a new rotary or indexed Setup the second. To use another machine, choose it from the **Machine** list in the Setup's inspector ([Add a Setup](Add-a-Setup)).

What the machine is used for today: FluutCAM warns when a spindle speed or feed is outside the machine's range (`param.tool_params.spindle_rpm.machine_range`, `param.tool_params.feed.machine_max`), checks that a rotary toolpath has a rotary axis and enough travel, and checks the Z travel before the part is turned (`setup.clearance.index_travel`; with the warning `cam.tool.length_missing` if the tool has no overall length, because the check then only knows the tool tip). The machine does not choose the post for the window or the command line; you do (see below).

## Built-in posts

| Id | For | Notes |
|---|---|---|
| `fluut/gcode` | Plain RS-274/NGC G-code | The base the others build on. Writes arcs as `G2` and `G3`, supports drilling cycles, writes a program in millimetres, ends with `M30`. |
| `fluut/linuxcnc` | LinuxCNC | Adds path blending (`G64`) and a tool length offset after each tool change (`G43 H1`). |
| `fluut/grbl` | Grbl 1.1 | No tool changer (a tool change is a stop with a message), no drilling cycles (they are written out as moves), no controller cutter compensation. |
| `fluut/linuxcnc-4x` | LinuxCNC with a rotary A axis along X | For rotary and indexed Setups. |

The Post stage's list offers the four built-in posts above. The command line takes any post id, including your own. See [Post G-code](Post-G-code). Other controllers (Fanuc, Mach3 and so on) have no built-in post yet: write one that extends `fluut/gcode`, convert the Vectric post processor you already use ([Import posts](Import-Posts)), or ask for one.

## Make your own

A definition is a text file in TOML. FluutCAM finds your files in the **project folder**: put them in these folders beside the project file.

```
my-part.fluut
machines/my-router.machine.toml
posts/my-controller.post.toml
tools/shop.toml
templates/...
```

The folders beside the project file load when you open the project. Their definitions are layered on top of the built-in ones, so a definition of yours can extend a built-in one. A folder of definitions shared between all your projects is not wired in yet ([Known limits](Known-Limits)); copy the files into each project for now.

### A machine

Start from a copy of a built-in machine. This is the machine file of the `flipside` reference project:

```toml
format  = "fluut.machine/1"
id      = "shop/router-4x"
version = "1.0"
name    = "Router with rotary axis"
post    = "shop/linuxcnc-4x@1"

[spindle]
rpm = [6000, 24000]

[feed]
max_cut   = 10000
max_rapid = 15000

[[axis.rotary]]
name        = "A"
parallel_to = "X"
centre      = [0, 0, 0]
continuous  = true

[tool_changer]
kind = "manual"
```

The `id` is what a Setup names: `machine = "shop/router-4x@1"`, where `@1` is the major version. `travel` gives `x`, `y` and `z` limits as `[min, max]` in millimetres. A rotary axis can have limits instead of `continuous = true`.

### A post

A post names its parent with `extends` and can add `mixins`, which are fragments that add a feature. This is the post of the `flipside` project: LinuxCNC plus the rotary A axis.

```toml
format  = "fluut.post/1"
id      = "shop/linuxcnc-4x"
version = "1.0"
name    = "LinuxCNC, 4 axes"
extends = "fluut/linuxcnc@1"
mixins  = ["fluut/mixin/rotary-a"]
```

You then change only what differs: a word's format, a block of lines at the start of the program, the tool-change sequence. Posts are plain declarative text, with `[PLACEHOLDER]` variables in the lines: no loops and no scripting. The full format, with every block and every placeholder, is in the [post processor specification](https://github.com/martinlong1978/fluutcam/blob/main/docs/spec/05-postprocessor-engine.md). Use `fluut post ... --post shop/linuxcnc-4x` to try it.

The built-in posts never change under you: a project names a post by major version (`@1`), and the program records the exact versions it used in its header.

## Not yet

- Importing machines from other CAM programs. Vectric posts import with `fluut posts import` ([Import posts](Import-Posts)) and Fusion tool libraries with `fluut tools import` ([Import tools](Import-Tools)).
- Making an imported post extend the closest built-in one (it is written as a standalone post).
- Choosing a post of your own from the Post stage's list (use the command line).
- A user folder of definitions shared between projects.
- More built-in posts (Fanuc, Mach3, Marlin and so on).

## Next

[Tools](Tools)
