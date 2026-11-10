# UAT: Post

Tests of writing G-code from the window. Part of the [user acceptance tests](UAT). Test files: the `block`, `boss`, `holes` and `shaft` projects, and the expected programs in `crates/fluut-cli/tests/golden/`. Work on copies. The comparison tests (UAT-PST-09 and UAT-PST-10) use the command line.

---

### UAT-PST-01 Post with LinuxCNC

- **Preconditions:** `block` open and calculated. In the **Post** stage the list shows `fluut/linuxcnc`.
- **Steps:**
  1. Click **Post**.
  2. In the save dialog check the name and folder offered: `block.ngc` in the project folder. Confirm.
- **Expected:** The status bar says "Wrote block.ngc". The file exists and starts with `(FluutCAM` and `(Project:`. It has `G21`, the Setup name `(Top)`, `G54`, a `T1 M6` tool change and `G43 H1`, and ends with `M30`.

![G-code written](images/posted.png)

- [ ] Pass

### UAT-PST-02 Post with Grbl

- **Preconditions:** `holes` open and calculated.
- **Steps:**
  1. Choose `fluut/grbl` in the Post stage's list.
  2. Click **Post**. The dialog offers `holes.gcode`. Confirm.
- **Expected:** The file is written. Tool changes read `M0 (Change to T1: ...)` (no `M6`). Drilling is written as separate moves (no `G81` or `G83`). The Messages tab warns that drilling was expanded (`param.drill.cycle.expanded`).
- [ ] Pass

### UAT-PST-03 Post with generic G-code

- **Preconditions:** `block` open and calculated.
- **Steps:**
  1. Choose `fluut/gcode` and click **Post**.
- **Expected:** The file is written, with `G21` and `M30`. It is plain RS-274 without the LinuxCNC tool length offset (`G43`).
- [ ] Pass

### UAT-PST-04 Change the output name

- **Preconditions:** `block` open.
- **Steps:**
  1. Click **Post**. In the save dialog type `first-cut.ngc` and confirm.
- **Expected:** The status bar says "Wrote first-cut.ngc" and the file is in the project folder under that name.
- [ ] Pass

### UAT-PST-05 Post calculates what is out of date

- **Preconditions:** `block` open, not calculated.
- **Steps:**
  1. Click **Post** without calculating.
- **Expected:** FluutCAM calculates the toolpaths, then writes the file. The status bar shows the calculation, then "Wrote block.ngc".
- [ ] Pass

### UAT-PST-06 A toolpath with an error blocks the post

- **Preconditions:** A project with a toolpath that fails: add a **2D pocket** in the window (see UAT-TP-12).
- **Steps:**
  1. Click **Post**. Confirm the dialog.
- **Expected:** The status bar turns red: "Cannot post the project" with the reason. No file is written (an earlier file of the same name is not changed). The Messages tab lists the error.
- [ ] Pass

### UAT-PST-07 Warnings do not block the post

- **Preconditions:** `block` open and calculated. Its toolpath "Cut out" has a warning.
- **Steps:**
  1. Click **Post**.
- **Expected:** The file is written. The warnings stay in the Messages tab.
- [ ] Pass

### UAT-PST-08 Post a project that has no Setup

- **Preconditions:** A new project with a model and no Setup.
- **Steps:**
  1. Open the **Post** stage and click **Post**. Confirm the dialog.
- **Expected:** **Post** is available. The file is written with only the header lines, the setup lines (`G21`, `G17 G40 G49 G80 G90 G94`, `G64 P0.01`) and `M30`, and no moves. The status bar says "Wrote ...".
- [ ] Pass

### UAT-PST-09 The output matches the reference program

- **Preconditions:** A copy of `block`. A shell in its folder, with `fluut` on the path.
- **Steps:**
  1. Run `fluut post block.fluut --post fluut/linuxcnc -o block.ngc --date 2026-11-01 --time 12:00:00`.
  2. Compare `block.ngc` with `crates/fluut-cli/tests/golden/block-linuxcnc.ngc`, for example with `diff`.
- **Expected:** The files are identical, byte for byte, except that the header names the version of your build. If the version line differs, only that line differs.
- [ ] Pass

### UAT-PST-10 Every reference program matches

- **Preconditions:** Copies of the reference projects.
- **Steps:**
  1. For each file in `crates/fluut-cli/tests/golden/`, run the matching `fluut post` command with `--date 2026-11-01 --time 12:00:00`. The post is in the file name: `linuxcnc`, `grbl` or `linuxcnc-4x` (the id `fluut/` plus that name).
  2. Compare.
- **Expected:** All thirteen outputs match their golden file, apart from the version line.
- [ ] Pass

### UAT-PST-11 A rotary part needs the 4-axis post

- **Preconditions:** A copy of `shaft`.
- **Steps:**
  1. Run `fluut post shaft.fluut --post fluut/linuxcnc-4x -o shaft.ngc`.
  2. Look for A moves in the file.
- **Expected:** The file is written and has `A` words. Using `fluut/linuxcnc` for the same project fails with `error post.emit: setup 0, toolpath 0: rotary moves need a post with rotary.enabled` and writes no file. In the window, choose `fluut/linuxcnc-4x` in the Post stage's list (UAT-PST-13).
- [ ] Pass

### UAT-PST-12 Cancel the save dialog

- **Preconditions:** `block` open and calculated.
- **Steps:**
  1. Click **Post** and cancel the save dialog.
- **Expected:** Nothing is written and the status bar does not say "Wrote". Nothing else changes.
- [ ] Pass

### UAT-PST-13 Rotary post from the list

- **Preconditions:** A copy of `shaft` open and calculated.
- **Steps:**
  1. In the Post stage choose `fluut/linuxcnc-4x` in the list.
  2. Click **Post** and confirm the dialog.
- **Expected:** The file is written and has `A` words. The list does not choose this post for you when the Setup is rotary: it starts at `fluut/linuxcnc`.
- [ ] Pass

### UAT-PST-14 Post the same project twice

- **Preconditions:** `block` open and calculated.
- **Steps:**
  1. Post to `a.ngc`, then to `b.ngc`.
  2. Compare the two files.
- **Expected:** The files are identical, apart from any date or time in the header. (The built-in posts write no date.)
- [ ] Pass

### UAT-PST-15 Read the program before you cut

- **Preconditions:** A written program from UAT-PST-01.
- **Steps:**
  1. Open the file in a text editor. Check the units, the work offset, the tool numbers, the feeds and the spindle speeds against the toolpath settings.
- **Expected:** They match. The tool numbers match the order of the tools. This test is a reminder, and is a habit to keep for every real job.
- [ ] Pass

### UAT-PST-16 The Z travel check needs the tool length

- **Preconditions:** A copy of `flipside` in a work folder with its `posts`, `machines` and `tools` folders. In `machines/router-4x.machine.toml` add the lines `[travel]` and `z = [-200, 500]` above `[spindle]`. The tool `flat-6` has no `overall_length`.
- **Steps:**
  1. Run `fluut calc flipside.fluut`.
  2. Add `overall_length = 60` under `flute_length` in `tools/shop.toml` and run it again.
- **Expected:** Step 1: one warning, `cam.tool.length_missing`, for tool `flat-6` (not one for each of the four toolpaths): the retract before the A turn is checked against the Z travel at the tool tip only. Step 2: no warning. A machine without a `[travel]` table never gives the warning.
- [ ] Pass

