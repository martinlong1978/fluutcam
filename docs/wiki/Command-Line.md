# Command line

`fluut` is the command-line program that comes with FluutCAM. It calculates, simulates and posts a project without opening the window, and runs scripts. Use it to post from a script or a build, to check a project on a computer with no screen, and to use the options the window does not have yet (any post, one Setup, fixed dates).

```
fluut calc <project> [--tools <file>]...
fluut sim <project> [--tools <file>]... [--error <mm>]
fluut post <project> --post <id> -o <file> [--tools <file>]... [--date <yyyy-mm-dd>] [--time <hh:mm:ss>]
fluut run <script> [<project>] [--tools <file>]... [--date <yyyy-mm-dd>] [--time <hh:mm:ss>] [-- <args>...]
fluut tools import <file> -o <library.toml> [--id <library id>] [--name <text>] [--force]
fluut update-check
fluut --version
fluut --help
```

`<project>` is a `.fluut` file. See [Install](Install) for how to put `fluut` on your path.

## Exit status

| Code | Meaning |
|---|---|
| 0 | It worked (warnings do not change this) |
| 1 | An error: the project would not open, a toolpath could not be calculated, the simulation found an error, the post failed |
| 2 | The command line was wrong |
| 3 | `update-check` only: this build has no update key |

A script (`fluut run`) sets its own status with `fluut.exit(n)`.

Warnings and errors print on the error stream as `warning [t3] code: message` or `error [t3] code: message`, where `t3` is the toolpath's id. [Diagnostics](Diagnostics) lists every code.

## `fluut calc`

Calculates every toolpath of the project and prints one line for each: its id and name, whether it was calculated, the length of its cutting and rapid moves, and an estimate of the machining time.

```
$ fluut calc block.fluut
t1 "Pocket": calculated; cut 3494.6 mm, rapid 324.6 mm, time 1:50
t2 "Pocket floor raster": calculated; cut 1161.7 mm, rapid 90.8 mm, time 0:39
t3 "Cut out": calculated; cut 1864.3 mm, rapid 167.6 mm, time 1:00
warning [t3] param.heights.below_stock: bottom -20.2 is below the stock bottom -20: the tool cuts the fixture
warning [t3] cam.linking.ramp_fallback: the chosen ramp did not fit at some entries; a fallback ramp was used
```

The time is `m:ss`, or `h:mm:ss` from an hour. The word after the name is `calculated`, `up to date`, `failed` or `disabled`. Exit status 1 if any toolpath failed.

A missing project file prints `error model.project.load: cannot read nothing.fluut: ...` and exits with 1.

## `fluut sim`

Calculates, then simulates every Setup and checks it. After the `calc` lines it prints one line per Setup:

```
s1 "Top": simulated at 0.2 mm in 1.13 s; 3 toolpaths, time 3:29; rest 58.9 mm³ on the part (up to 8.000 mm), 618.5 mm³ off it
```

It prints the findings (gouges, holder collisions and so on) and exits with 1 if one is an error. `--error <mm>` sets the accuracy: the simulation's grid is twice this (`--error 0.1` gives 0.2 mm). A smaller number is more exact and slower. Rotary Setups are not simulated yet.

## `fluut post`

Calculates what is out of date and writes the G-code.

```
fluut post block.fluut --post fluut/linuxcnc -o block.ngc
```

- `--post <id>` is any post id: a built-in (`fluut/linuxcnc`, `fluut/grbl`, `fluut/gcode`, `fluut/linuxcnc-4x`) or one from the project's `posts/` folder. See [Machines and posts](Machines-and-Posts).
- `-o <file>` is the output. If the post asks for more than one file, the extra files get `.2`, `.3` before the extension. The program prints `wrote <file>` for each.
- `--date` and `--time` set the date and time that the post may put in the header. Without them the current UTC time is used. Fix them to get the same output byte for byte, for example in tests.

A wrong post id exits with 1 and says `error post.load: nope/x: definition not found: nope/x`. A missing `--post` or `-o` exits with 2.

Rotary and indexed Setups need `fluut/linuxcnc-4x` (or a post of your own that extends it).

## `fluut run`

Runs a [script](Scripting) without the window.

```
fluut run job.rhai block.fluut -- out.ngc
```

The script can read and write the project's folder and the paths you list after `--`, and can load modules from the project's `scripts/` folder. Arguments after `--` reach the script as `fluut.args`. The project can also be given as `--project <file>`. See [Scripting](Scripting).

## `--tools <file>`

Loads an extra tool library, after the project's own `tools/` folder. Give it more than once to load more. Use it to try a tool library without copying it into the project. See [Tools](Tools).

## `fluut tools import`

Converts a Fusion tool library (`.json`) to a FluutCAM tool library file. It writes a new file and refuses to replace one unless you give `--force`. It prints a warning for every tool or value it changed or left out. Exit status 0 when the file was written, 1 for an error, 2 for a wrong command line. See [Import tools](Import-Tools).

## `fluut update-check`

Asks once for the signed list of releases and says whether a newer version exists. It runs only when you type it. Nothing runs it automatically, and it downloads and installs nothing.

```
FluutCAM 0.1.0 is up to date.
```

or

```
Version 0.2.0 is available (2026-12-01). You have 0.1.0.
Release notes: ...
Download page: ...
```

Exit status 0 for both, 1 if the check failed, 3 if this build has no update key. See [Settings and updates](Settings-and-Updates).

## `fluut --version`

Prints the version and the build, for example `fluut 0.1.0 (4fd4ea9cdd48)`. Put this in a bug report.
