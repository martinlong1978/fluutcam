# UAT: Command line

Tests of the `fluut` program. Part of the [user acceptance tests](UAT). Test files: copies of the `block`, `boss` and `shaft` projects in a work folder. Open a terminal in the work folder with `fluut` on the path (see [Install](Install)). After each command check its exit status: `echo $?` in a Unix shell, `echo %ERRORLEVEL%` in `cmd`, `$LASTEXITCODE` in PowerShell. See [Command line](Command-Line) for the commands.

---

### UAT-CLI-01 Version

- **Steps:**
  1. Run `fluut --version`.
- **Expected:** One line, `fluut 0.1.0 (` followed by a build id and `)`. Exit status 0. Write the line on the record sheet.
- [ ] Pass

### UAT-CLI-02 Help

- **Steps:**
  1. Run `fluut --help`.
- **Expected:** The usage lines for `calc`, `sim`, `post`, `run` and `update-check`. Exit status 0.
- [ ] Pass

### UAT-CLI-03 Calculate

- **Preconditions:** A copy of `block`.
- **Steps:**
  1. Run `fluut calc block.fluut`.
- **Expected:** Three lines, for "Pocket", "Pocket floor raster" and "Cut out", each saying `calculated` with cut and rapid lengths and a time. Pocket reads `cut 3494.6 mm, rapid 324.6 mm, time 1:50`. Then two warnings for t3 (`param.heights.below_stock` and `cam.linking.ramp_fallback`). Exit status 0, because warnings do not fail the command.
- [ ] Pass

### UAT-CLI-04 Simulate

- **Preconditions:** A copy of `block`.
- **Steps:**
  1. Run `fluut sim block.fluut`.
- **Expected:** The `calc` lines, then a line starting `s1 "Top": simulated at 0.2 mm`, with 3 toolpaths and a time of 3:29, and the rest figures (`rest 58.9 mm³ on the part`). Exit status 0.
- [ ] Pass

### UAT-CLI-05 Simulate with a finer grid

- **Preconditions:** A copy of `block`.
- **Steps:**
  1. Run `fluut sim block.fluut --error 0.1`.
- **Expected:** The simulation line says `simulated at 0.2 mm` (the grid is twice the error). Run it with `--error 0.2` too: the line says `simulated at 0.4 mm`, and the rest figures differ a little.
- [ ] Pass

### UAT-CLI-06 Post

- **Preconditions:** A copy of `block`.
- **Steps:**
  1. Run `fluut post block.fluut --post fluut/linuxcnc -o block.ngc`.
- **Expected:** The calculation lines, then `wrote block.ngc`. Exit status 0. The file starts with `(FluutCAM 0.1.0)`.
- [ ] Pass

### UAT-CLI-07 Each built-in post

- **Preconditions:** A copy of `block`.
- **Steps:**
  1. Run `fluut post` for `fluut/gcode`, `fluut/grbl` and `fluut/linuxcnc`, each with its own output name.
- **Expected:** Three files are written, exit status 0 each time. The Grbl file has `M0 (Change to T1` lines and no `M6`. The other two have `T1 M6`.
- [ ] Pass

### UAT-CLI-08 Fixed date and time

- **Preconditions:** A copy of `block`.
- **Steps:**
  1. Run `fluut post block.fluut --post fluut/linuxcnc -o a.ngc --date 2026-11-01 --time 12:00:00`, then again with `-o b.ngc`.
  2. Compare `a.ngc` and `b.ngc`.
- **Expected:** The files are identical, byte for byte.
- [ ] Pass

### UAT-CLI-09 Run a script

- **Preconditions:** A file `t.rhai` with the lines `print("hello");` and `fluut.exit(5);`.
- **Steps:**
  1. Run `fluut run t.rhai block.fluut`.
- **Expected:** `hello` prints and the exit status is 5: a script sets its own status with `fluut.exit`.
- [ ] Pass

### UAT-CLI-10 A script with a syntax error

- **Preconditions:** A file `bad.rhai` containing `let x = ;`.
- **Steps:**
  1. Run `fluut run bad.rhai block.fluut`.
- **Expected:** An error on the error stream that names `bad.rhai`, the line and the position, for example `bad.rhai:1: Unexpected ';' (line 1, position 9)`. Exit status 1.
- [ ] Pass

### UAT-CLI-11 A script that posts

- **Preconditions:** A script from the [Scripting](Scripting) page that opens the project, calculates and posts to the path given after `--`.
- **Steps:**
  1. Run `fluut run job.rhai block.fluut -- out.ngc`.
- **Expected:** `out.ngc` is written. Writing to a path not listed after `--` and outside the project folder is refused.
- [ ] Pass

### UAT-CLI-12 Missing project

- **Steps:**
  1. Run `fluut calc nothing.fluut`.
- **Expected:** `error model.project.load: cannot read nothing.fluut: ...` on the error stream, followed by the system's reason. Exit status 1.
- [ ] Pass

### UAT-CLI-13 Missing post options

- **Steps:**
  1. Run `fluut post block.fluut`.
- **Expected:** The message `fluut: post needs --post <id> and -o <file>` and the usage lines. Exit status 2.
- [ ] Pass

### UAT-CLI-14 Unknown post

- **Steps:**
  1. Run `fluut post block.fluut --post nope/x -o x.ngc`.
- **Expected:** `error post.load: nope/x: definition not found: nope/x`. Exit status 1. No `x.ngc` is written.
- [ ] Pass

### UAT-CLI-15 No project given

- **Steps:**
  1. Run `fluut calc` with no project.
- **Expected:** `fluut: no project file`, the usage lines, and exit status 2.
- [ ] Pass

### UAT-CLI-16 Missing tool library

- **Steps:**
  1. Run `fluut calc block.fluut --tools nope.toml`.
- **Expected:** `error model.tools.load: nope.toml: ...` and exit status 1.
- [ ] Pass

### UAT-CLI-17 A rotary part

- **Preconditions:** A copy of `shaft`.
- **Steps:**
  1. Run `fluut calc shaft.fluut`.
  2. Run `fluut sim shaft.fluut`.
  3. Run `fluut post shaft.fluut --post fluut/linuxcnc-4x -o shaft.ngc`.
- **Expected:** The calculation lists five calculated toolpaths. `sim` prints `error sim.setup.rotary: ...` saying that Setup s1 is rotary and rotary Setups are not simulated yet, and does not crash; check its exit status and record it. `post` writes `shaft.ngc` with A words.
- [ ] Pass

### UAT-CLI-18 Update check

- **Steps:**
  1. Run `fluut update-check`.
- **Expected:** In a build with no update key (every build for now): `fluut: no update key in this build` and exit status 3. In a build with the key, it states whether a newer version exists and exits 0, or exits 1 if the check failed. The program does not download or install anything.
- [ ] Pass

### UAT-CLI-19 Run from another folder

- **Steps:**
  1. Run `fluut calc` with the full path to the project from a different folder.
  2. Run `fluut post <full path> --post fluut/gcode -o out.ngc` from that folder.
- **Expected:** Both work. `out.ngc` is written in the folder you ran from.
- [ ] Pass

### UAT-CLI-20 Update check channel option

- **Steps:**
  1. Run `fluut update-check --channel nightly`.
  2. Run `fluut update-check --channel`.
  3. Run `fluut update-check --channel beta`.
- **Expected:** Steps 1 and 2: `fluut: --channel needs stable or beta` followed by the usage lines, exit status 2, and no network use. Step 3: as UAT-CLI-18 (in a build with no update key: `fluut: no update key in this build`, status 3; with a key, the answer for the beta list or a failure message with status 1 if no beta release exists yet).
- [ ] Pass
