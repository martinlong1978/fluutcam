# UAT: Import posts

Tests of `fluut posts import` (UAT-PPI-01 to 10). Part of the [user acceptance tests](UAT). Test files: you make them from the text below (they are hand-written, not from any product). Work in an empty folder with `fluut` on the path, and with a copy of the `plate` reference project for UAT-PPI-03. See [Import posts](Import-Posts).

If you have Vectric software, UAT-PPI-10 also uses one of its own `.pp` files and a G-code file it wrote with that post.

**File to make, `router.pp`:**

```
+ UAT test post, written by hand.
POST_NAME = "UAT router (mm)"
FILE_EXTENSION = "tap"
UNITS = "MM"
LINE_ENDING = "[13][10]"
SUBSTITUTE = "({)}"
LINE_NUMBER_START = 10
LINE_NUMBER_INCREMENT=10
LINE_NUMBER_MAXIMUM = 9990

VAR LN = [N|A|N|1.0]
VAR RPM = [S|A|S|1.0]
VAR FEED = [F|C||1.1]
VAR XPOS = [X|C|X|1.3]
VAR YPOS = [Y|C|Y|1.3]
VAR ZPOS = [Z|C|Z|1.3]
VAR ICEN = [I|A|I|1.3]
VAR JCEN = [J|A|J|1.3]
VAR ZHOME = [ZH|A|Z|1.3]

begin HEADER
"[N] ([TP_FILENAME] for [TOOLNAME])"
"[N] G21 G90 G17"
"[N] T[T] M6"
"[N] [S] M3"

begin TOOLCHANGE
"[N] M5"
"[N] T[T] M6"

begin NEW_SEGMENT
"[N] ([TOOLPATH_NAME])"
"[N] [S] M3"

begin RAPID_MOVE
"[N] G0 [X] [Y] [Z]"

begin FIRST_FEED_MOVE
"[N] G1 [X] [Y] [Z] F[F]"

begin FEED_MOVE
"[N] G1 [X] [Y] [Z]"

begin FIRST_CW_ARC_MOVE
"[N] G2 [X] [Y] [I] [J] F[F]"

begin CW_ARC_MOVE
"[N] G2 [X] [Y] [I] [J]"

begin FIRST_CCW_ARC_MOVE
"[N] G3 [X] [Y] [I] [J] F[F]"

begin CCW_ARC_MOVE
"[N] G3 [X] [Y] [I] [J]"

begin FOOTER
"[N] M5"
"[N] G0 [ZH]"
"[N] M30"
```

---

### UAT-PPI-01 Import a post

- **Steps:**
  1. Run `fluut posts import router.pp -o router.post.toml`. Check the exit status.
- **Expected:** One warning, `import.pp.placeholder`, naming `[TP_FILENAME]`. The last line says ``router.post.toml: converted `UAT router (mm)` as post vectric/router (11 of 11 blocks mapped, 9 words, 1 warnings)``. Exit status 0. The file `router.post.toml` exists.
- [ ] Pass

### UAT-PPI-02 What the file holds

- **Preconditions:** UAT-PPI-01 done.
- **Steps:**
  1. Open `router.post.toml` in a text editor.
- **Expected:** `id = "vectric/router"`, `origin = "imported"`, `extension = "tap"`, `line_ending = "crlf"`, `substitute_values` with `(` to `{` and `)` to `}`. `[words.X]` with `when = "changed"` and `decimals = 3`. `[blocks.tool_change]` lines with `when = "!state.first_tool"`. The header's first line reads `[N] ( for [TOOLNAME])`.
- [ ] Pass

### UAT-PPI-03 Post a project with it

- **Preconditions:** UAT-PPI-01 done; a copy of the `plate` project.
- **Steps:**
  1. Make a folder `posts` beside `plate.fluut` and copy `router.post.toml` into it.
  2. Run `fluut post plate.fluut --post vectric/router -o plate.tap`.
  3. Open `plate.tap`.
- **Expected:** Exit status 0. After FluutCAM's comment lines, the lines are numbered `N10`, `N20`, … The program starts `N10 ( for …)` with the tool's name, then `N20 G21 G90 G17`, `N30 T1 M6` and `N40 S18000 M3`, and the first move `N50 G0 X35.000 Y35.000 Z10.000`. Coordinates have three decimals, feeds one (`F600.0`). The file has Windows line ends (CR LF). It ends with `M5`, a `G0 Z…` and `M30`.
- [ ] Pass

### UAT-PPI-04 Refuse a post that would scale moves

- **Steps:**
  1. Copy `router.pp` to `nounits.pp` and delete the `UNITS` line.
  2. Run `fluut posts import nounits.pp -o nounits.post.toml`.
- **Expected:** ``error import.pp.units: no `UNITS`: …``. Exit status 1. No `nounits.post.toml` is written.
- [ ] Pass

### UAT-PPI-05 Refuse inverse-time and speed-step posts

- **Steps:**
  1. Copy `router.pp` to `g93.pp` and add a line `INVERSE_TIME_MODE = YES` after `UNITS`. Import it.
  2. Copy `router.pp` to `steps.pp` and add a line `SPINDLE_SPEED_RANGE = 1 15 5000 20000` after `UNITS`. Import it.
- **Expected:** 1: `error import.pp.inverse_time`. 2: `error import.pp.spindle_range`. Both exit with 1 and write nothing.
- [ ] Pass

### UAT-PPI-06 A base post

- **Preconditions:** `router.pp` in the folder.
- **Steps:**
  1. Make `child.pp` with these three lines: `POST_NAME = "UAT child"`, `POST_BASE = "router.pp"`, `VAR XPOS = [X|C|X|1.4]`.
  2. Run `fluut posts import child.pp -o child.post.toml`.
  3. Delete `router.pp` (or rename it) and run the command again with `--force`.
- **Expected:** 2: exit status 0, the line says ``converted `UAT child` ``; in the file `[words.X]` has `decimals = 4` and the blocks are those of `router.pp`. 3: ``error import.pp.post_base: the base post `router.pp` was not found …``, exit status 1.
- [ ] Pass

### UAT-PPI-07 No overwrite without --force

- **Preconditions:** UAT-PPI-01 done.
- **Steps:**
  1. Run `fluut posts import router.pp -o router.post.toml` again.
  2. Run it again with `--force`.
- **Expected:** 1: `error import.output.exists`, exit status 1, the file unchanged. 2: exit status 0.
- [ ] Pass

### UAT-PPI-08 Ids and bad arguments

- **Steps:**
  1. Run `fluut posts import router.pp -o a.post.toml --id shop/router`.
  2. Run `fluut posts import router.pp -o b.post.toml --id "Shop Router"`.
  3. Run `fluut posts import router.pp` (no `-o`).
  4. Run `fluut posts import router.cps -o c.post.toml`.
- **Expected:** 1: exit 0, `as post shop/router`, and `id = "shop/router"` in the file. 2: `error import.post.bad_id`, exit 1. 3: `no output file (-o)` and the usage text, exit 2. 4: `error import.format.unsupported`, exit 1.
- [ ] Pass

### UAT-PPI-09 Wrapped rotary output

- **Steps:**
  1. Copy `router.pp` to `wrap.pp` and add a line `ROTARY_WRAP_Y = "-A"` after `UNITS`. Import it.
  2. Open the new file.
- **Expected:** A warning `import.pp.rotary_wrap` that says A is written next to Y with the sign reversed and the rotary axis must lie along X. In the file: `[rotary]` with `enabled = true`, `[words.A]` with `prefix = "A"` and `scale = -1.0`, and the rapid line `[N] G0 [X] [Y] [A] [Z]`.
- [ ] Pass

### UAT-PPI-10 A real Vectric post (optional)

- **Preconditions:** Vectric software on the computer; one of its `.pp` files for your machine, and a G-code file it wrote with that post.
- **Steps:**
  1. Import the `.pp` file into a project's `posts/` folder.
  2. Post a small job of yours with it.
  3. Compare the start, the end, a tool change and the number formats with the G-code Vectric wrote.
- **Expected:** The import exits with 0 (or is refused with a reason from [Import posts](Import-Posts)). Each warning names something the page lists. The number formats, the line numbers and the program's start and end lines match Vectric's; FluutCAM's comment lines at the top and full axis words after a tool change are expected differences. Note anything else on the record sheet.
- [ ] Pass
