# UAT: Import posts

Tests of `fluut posts import` (UAT-PPI-01 to 10) and of **Import post…** in the window (UAT-PPI-11 to 16). Part of the [user acceptance tests](UAT). Test files: you make them from the text below (they are hand-written, not from any product). Work in an empty folder with `fluut` on the path, and with a copy of the `plate` reference project for UAT-PPI-03. See [Import posts](Import-Posts).

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

## In the window

UAT-PPI-11 to 16 use the **Import post…** command on the Library tab. They need the FluutCAM window, `router.pp` from above in an empty folder, and (for 12 to 16) a copy of the `plate` reference project with a `posts` folder beside `plate.fluut`. See [Import posts](Import-Posts).

### UAT-PPI-11 Import a post in the window, without a project

- **Preconditions:** FluutCAM is started and no project is open.
- **Steps:**
  1. Open the **Library** tab and click **Import post…**.
  2. In the file dialog, check that only `.pp` files are offered, and choose `router.pp`.
  3. In the Save dialog, check that the offered name is `router.post.toml`, and save it in the working folder.
  4. Read the result window.
- **Expected:** A brief **Importing router.pp…** window may show. The result window says ``Wrote post vectric/router to …router.post.toml.`` and lists `Post id: vectric/router`, `Post name: UAT router (mm)`, `Settings mapped`, `Blocks mapped: 11 of 11`, `Words written: 9` and `Warnings: 1`. Under **Warnings** there is one line, a **Warning** about `[TP_FILENAME]` with the code `[import.pp.placeholder]`. The window says to open a project to use the post, and has no **Use this post** button. There is no text box for a path anywhere. The file `router.post.toml` exists, the same as `fluut posts import` writes in UAT-PPI-01.
- [ ] Pass

### UAT-PPI-12 Use the post in the project

- **Preconditions:** `plate.fluut` is open, with the `posts` folder beside it.
- **Steps:**
  1. Click **Import post…**, choose `router.pp`, and in the Save dialog check that it starts in the project's `posts` folder. Save as `router.post.toml` there.
  2. In the result window, read the last paragraph and click **Use this post**.
  3. Close the window. Open the **Post** stage and open the post list.
  4. Click **Post** and look at the file name the dialog offers. Save it.
- **Expected:** 2: the window says the post is in this project's posts folder, then ``Post now uses this post.`` (nothing in the project is changed: the title bar shows no unsaved changes). 3: the list shows `vectric/router` as the chosen post, below the four built-in posts. 4: the dialog offers `plate.tap`. The file starts like the output of UAT-PPI-03 and has CR LF line ends, with no restart of FluutCAM.
- [ ] Pass

### UAT-PPI-13 A post saved outside the posts folder

- **Preconditions:** `plate.fluut` is open.
- **Steps:**
  1. Click **Import post…**, choose `router.pp`, and save the post in a folder that is not `posts` beside the project (for example the project folder itself).
- **Expected:** The result window shows the counts, and then says the post is outside this project's posts folder, so FluutCAM does not find it. There is no **Use this post** button, and the Post stage's list does not show `vectric/router`. Moving the file into `posts` and opening the project again shows it in the list.
- [ ] Pass

### UAT-PPI-14 Cancel, overwrite and the source file

- **Steps:**
  1. Click **Import post…** and cancel the file dialog. Click it again, choose `router.pp` and cancel the Save dialog.
  2. Click it again, choose `router.pp`, and in the Save dialog choose a `.post.toml` file that exists. Confirm replacing it.
  3. Repeat step 2 but, in the Save dialog, choose `router.pp` itself and confirm replacing it.
  4. Click it again, choose `router.pp`, and in the Save dialog type a name with no extension, such as `mine`.
- **Expected:** 1: nothing happens and no file is written. 2: the result window shows the counts and the file is replaced by the converted post. 3: the window says ``Nothing was imported.`` with `[import.output.is_input]`, and `router.pp` is unchanged. 4: the file is saved as `mine.post.toml`.
- [ ] Pass

### UAT-PPI-15 Files that are refused

- **Preconditions:** a file `shop.cps` (any text, for example one line) and `nounits.pp` from UAT-PPI-04.
- **Steps:**
  1. Click **Import post…** and choose `shop.cps`.
  2. Click **Import post…**, choose `nounits.pp` and save it as `nounits.post.toml`.
- **Expected:** 1: no Save dialog opens. The window says ``Nothing was imported.`` with an **Error** line, ``Autodesk .cps posts are programs; FluutCAM does not import them yet …`` and the code `[import.format.unsupported]`. 2: ``Nothing was imported.`` with an **Error** line and the code `[import.pp.units]`, and no `nounits.post.toml` is written. **Close** closes the window each time.
- [ ] Pass

### UAT-PPI-16 A base post, and the command waits for a running job

- **Preconditions:** `router.pp` and `child.pp` from UAT-PPI-06 in one folder; `plate.fluut` open.
- **Steps:**
  1. Click **Import post…**, choose `child.pp` and save it in the `posts` folder.
  2. In the **Toolpaths** stage click **Calculate all**, and at once open the Library tab and look at **Import post…**.
- **Expected:** 1: the result window lists ``Base files merged: 1`` and the post id `vectric/child`. 2: while the calculation runs, **Import post…** is greyed out; it works again when the calculation has finished.
- [ ] Pass
