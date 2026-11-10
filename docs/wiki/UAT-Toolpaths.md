# UAT: Toolpaths

Tests of adding, editing and calculating toolpaths. Part of the [user acceptance tests](UAT). Test files: the `block`, `ridge`, `plate2d`, `shaft` and `holes` projects. Work on copies.

A 2-D toolpath you add in the window reports an error until you pick its faces or curves on the model (UAT-TP-21). The reference projects already have their geometry set.

---

### UAT-TP-01 Add a 3-D toolpath

- **Preconditions:** A new project with `ridge/ridge.step` imported (copy the file from the `ridge` project) and a 3-axis Setup added.
- **Steps:**
  1. Open the **Toolpaths** stage and click **Parallel** in the 3D gallery.
- **Expected:** "Parallel 1" appears in the Browser under the Setup, selected, with a hollow grey circle (not calculated). The status bar says it was added. The inspector shows the five tabs Tool, Geometry, Heights, Passes and Linking, and the tool is the 6 mm ball end mill.

![A new toolpath](images/toolpath-added.png)

- [ ] Pass

### UAT-TP-02 The galleries list all 22 types

- **Preconditions:** A project with a 3-axis Setup selected, and another with a rotary Setup (`shaft`).
- **Steps:**
  1. Open the **Toolpaths** stage with the 3-axis Setup selected. Count the buttons.
  2. Select the rotary Setup and count again.
- **Expected:** For the 3-axis Setup: the 2D gallery (8 types), Holes (Drill), 3D (8 types) and Utility (Manual NC): 18 buttons. For the rotary Setup: the Rotary gallery (4 types) and Utility (Manual NC), and no 2D, Holes or 3D gallery. Together they cover the 22 types of [Toolpath types](Toolpath-Types). An indexed Setup shows the same galleries as a 3-axis one.

![The Toolpaths stage](images/ribbon-toolpaths.png)

- [ ] Pass

### UAT-TP-03 Add toolpath needs a Setup

- **Preconditions:** A project with a model and no Setup.
- **Steps:**
  1. Open the **Toolpaths** stage and look at the toolpath buttons.
- **Expected:** They are greyed out. After a Setup is added they are available.
- [ ] Pass

### UAT-TP-04 The new toolpath goes into the selected Setup

- **Preconditions:** A project with two Setups; select "Setup 2".
- **Steps:**
  1. Click **Parallel** in the 3D gallery.
- **Expected:** The toolpath is listed under "Setup 2", at the end of its list. With nothing selected the toolpath goes into the first Setup.
- [ ] Pass

### UAT-TP-05 Calculate a toolpath

- **Preconditions:** UAT-TP-01 done.
- **Steps:**
  1. Click **Calculate all**.
  2. Watch the Browser and the status bar.
- **Expected:** During the job the status bar shows a progress bar and "Calculating Parallel 1 (1 of 1)", the toolpath's icon shows a filling ring, and **Calculate all** and **Open** are greyed out. At the end the status bar says "Calculated 1 toolpaths" (or the matching wording), the icon is a green filled circle, and the path is drawn over the model.

![A calculated toolpath](images/calculated.png)

- [ ] Pass

### UAT-TP-06 Edit a setting and see it marked out of date

- **Preconditions:** UAT-TP-05 done.
- **Steps:**
  1. Select "Parallel 1". Open the **Passes** tab.
  2. Change **Stepover** to a different value and press Enter.
  3. Look at the Browser icon.
- **Expected:** The setting shows **Reset to default**. The toolpath icon becomes an amber ring with a dot (out of date). The title shows `*`. Clicking **Calculate all** makes it green again.
- [ ] Pass

### UAT-TP-07 A setting that only changes the G-code does not mark it out of date

- **Preconditions:** UAT-TP-05 done and the toolpath is green.
- **Steps:**
  1. On the **Tool** tab change the feed rate (cutting feed) and press Enter.
  2. Look at the icon.
- **Expected:** The icon stays green. The title shows `*`.
- [ ] Pass

### UAT-TP-08 Reset to default

- **Preconditions:** UAT-TP-06 done.
- **Steps:**
  1. Click **Reset to default** by the changed setting.
- **Expected:** The setting returns to its default (shown in grey) and the button goes away. The toolpath is out of date again until calculated.
- [ ] Pass

### UAT-TP-09 Type an expression

- **Preconditions:** A toolpath is selected.
- **Steps:**
  1. In a length setting, type `1/4in` and press Enter.
- **Expected:** The setting accepts it, and shows its value in the project unit (about 6.35 in a millimetre project).
- [ ] Pass

### UAT-TP-10 Type something invalid

- **Preconditions:** A toolpath is selected.
- **Steps:**
  1. In a number box type `abc` and press Enter.
- **Expected:** A red message appears under the box, and the value is not changed. No new undo step is made. The program does not crash.
- [ ] Pass

### UAT-TP-11 Undo a setting change

- **Preconditions:** UAT-TP-06 done.
- **Steps:**
  1. Press Ctrl+Z (or click **Undo**).
- **Expected:** The setting returns to its previous value, and the icon returns to its previous state.
- [ ] Pass

### UAT-TP-12 A 2-D toolpath added in the window reports a missing selection

- **Preconditions:** A project with a model and a 3-axis Setup.
- **Steps:**
  1. Click **2D pocket** in the 2D gallery.
  2. Click **Calculate all**.
- **Expected:** The toolpath gets a red filled triangle. Its message, in the Messages tab, is that you must select drive curves or drive faces. Nothing else is changed. UAT-TP-21 fixes it by picking.

![Messages](images/messages.png)

- [ ] Pass

### UAT-TP-13 A rotary type cannot be added to a 3-axis Setup

- **Preconditions:** A project with a model and a 3-axis Setup selected.
- **Steps:**
  1. Open the **Toolpaths** stage and look for the Rotary gallery.
- **Expected:** There is no Rotary gallery. (A project file that puts a rotary type in a 3-axis Setup is calculated with the error `param.type.setup_kind`, a red triangle and a message that the toolpath runs in rotary Setups only.)
- [ ] Pass

### UAT-TP-14 Heights in the wrong order

- **Preconditions:** A calculated 3-D toolpath in a project.
- **Steps:**
  1. On the **Heights** tab set the **Retract** offset so the retract height is lower than the feed height (for example reference Stock top, offset `-10`).
  2. Click **Calculate all**.
- **Expected:** The toolpath gets a red triangle and a message that the heights must satisfy clearance, retract, feed, top and bottom in that order.
- [ ] Pass

### UAT-TP-15 A warning does not stop the toolpath

- **Preconditions:** A project from the `block` folder, calculated.
- **Steps:**
  1. Select "Cut out". Look at its icon and its messages.
- **Expected:** "Cut out" shows an amber triangle outline. The Messages tab lists two warnings for it, `param.heights.below_stock` (the bottom is below the stock bottom: the tool cuts the fixture) and `cam.linking.ramp_fallback` (a fallback ramp was used), and it still has a path.
- [ ] Pass

### UAT-TP-16 Cancel a calculation

- **Preconditions:** `rough3d/rough3d.fluut` open (3-D roughing takes a few seconds).
- **Steps:**
  1. Click **Calculate all**.
  2. Click **Cancel** while it runs.
- **Expected:** The status bar says "Cancelled" after the current toolpath ends. Toolpaths that finished are green and keep their paths. The ones that did not start are not calculated. Calculate all again finishes the rest.
- [ ] Pass

### UAT-TP-17 Calculate all the reference parts

- **Preconditions:** Copies of the reference projects.
- **Steps:**
  1. Open `block`, `ridge`, `rough3d`, `dome`, `holes`, `plate`, `plate2d`, `vcarve` and `boss` in turn. For each one click **Calculate all**.
- **Expected:** Every toolpath ends green or amber. None ends red. The status bar never shows "Failed:". No toolpath is empty, except where a note in the project says so.
- [ ] Pass

### UAT-TP-18 A toolpath is calculated once

- **Preconditions:** `block` open and calculated.
- **Steps:**
  1. Click **Calculate all** again without changing anything.
- **Expected:** The job ends at once: nothing needs calculating (the status bar says "Calculated 0 toolpaths" or similar). The paths do not change.
- [ ] Pass

### UAT-TP-19 Results are not kept after closing

- **Preconditions:** `block` calculated and saved.
- **Steps:**
  1. Open `block.fluut` again (click **Open**; discard changes if asked).
- **Expected:** The toolpaths show as not calculated (hollow grey circles). Calculating again gives the same result. (Saving results is planned: see [Known limits](Known-Limits).)
- [ ] Pass

### UAT-TP-20 The number box explains what it accepts

- **Preconditions:** A toolpath is selected.
- **Steps:**
  1. Hover over a number box.
- **Expected:** A tooltip says: type a value or an expression, for example 6/2, 1/4in or tool.diameter*0.4. (A tooltip that explains each setting is not in the window yet; the meaning of every setting is in [Toolpath types](Toolpath-Types).)
- [ ] Pass

### UAT-TP-21 Pick a face for a pocket

- **Preconditions:** A new project with `plate2d/plate2d.step` imported and a 3-axis Setup added.
- **Steps:**
  1. Click **2D pocket**. Open the **Geometry** tab. Next to **Drive faces** click **Pick**.
  2. Read the banner at the top of the viewport. Move the pointer over the model.
  3. Click the floor of the pocket.
  4. Click the same face again.
  5. Click it once more, then click **Done**.
  6. Click **Calculate all**.
- **Expected:** The banner says "Picking Drive faces". The simulated stock hides. The face under the pointer lights up. After step 3 the face is tinted and the inspector says "1 selected" and lists its id. After step 4 it is off the list. After step 5 pick mode ends and the banner goes. The pocket calculates with a green circle.

![Pick mode](images/pick-mode.png)

- [ ] Pass

### UAT-TP-22 Leave pick mode with Esc, and undo a pick

- **Preconditions:** UAT-TP-21 step 1 done, in pick mode.
- **Steps:**
  1. Click two faces. Click **Undo**.
  2. Press **Esc**.
  3. Click **Pick** again, then click another toolpath in the Browser.
- **Expected:** Undo takes back the last pick only: "1 selected". Esc leaves pick mode. Selecting another row also leaves pick mode.
- [ ] Pass

### UAT-TP-23 Remove and Clear

- **Preconditions:** A pocket with two faces picked.
- **Steps:**
  1. Click **Remove** beside one face.
  2. Click **Clear**.
- **Expected:** Remove takes that one face off. Clear empties the list: "0 selected". Each is one undo step.
- [ ] Pass

### UAT-TP-24 Drill and bore take cylinders only

- **Preconditions:** A new project with `holes/holes.step` imported and a 3-axis Setup added. (The `holes` project also works.)
- **Steps:**
  1. Click **Drill**. On the **Geometry** tab click **Pick** beside **Drive faces**.
  2. Move the pointer over a flat face, then over the wall of a hole. Click the hole wall.
  3. Leave pick mode. Add a **Bore** and look at what it picks.
- **Expected:** Flat faces do not light up and a click on one adds nothing. A hole wall lights up and is added. Bore behaves the same.
- [ ] Pass

### UAT-TP-25 Pick curves and add a closed loop

- **Preconditions:** A new project with `plate2d/plate2d.step` imported and a 3-axis Setup. A **2D contour** added.
- **Steps:**
  1. On the **Geometry** tab click **Pick** beside **Drive curves**.
  2. Click one edge of the plate's outline.
  3. Click **Add closed loop (n edges)**.
- **Expected:** After step 2 the inspector offers "Add closed loop (n edges)", where n is the number of edges still to add. After step 3 the whole loop is listed and the offer goes. Picking a flat face in this field also works.
- [ ] Pass

### UAT-TP-26 Choose a tool

- **Preconditions:** A project with a toolpath (UAT-TP-01).
- **Steps:**
  1. Open the **Tool** tab. Open the **Tool** list.
  2. Pick another tool, for example the 6 mm flat end mill.
  3. Click **Undo**.
- **Expected:** The list shows lines like "Starter tools: Flat 6 mm, 6 mm flat end mill". After step 2 the toolpath's detail line in the inspector names the new tool and the toolpath is out of date. A tool that does not suit the toolpath shows a message under the list. After step 3 the first tool is back.

![The Tool tab](images/toolpath-tool.png)

- [ ] Pass

### UAT-TP-27 Edit a height

- **Preconditions:** A toolpath selected, on the **Heights** tab.
- **Steps:**
  1. Open the list of **Retract**. Choose "Model top".
  2. Type `4` in its offset box and press Enter.
  3. Open the list of **Feed**. Look at the choices.
  4. Open the list of **Retract** again.
- **Expected:** The grey line under Retract shows its default and a **Reset to default** link appears. The Feed list has "Top height". The Retract list does not. The offset takes a negative number (`-2`). Each change is one undo step and marks the toolpath out of date.

![The Heights tab](images/toolpath-heights.png)

- [ ] Pass

### UAT-TP-28 Heights in inches

- **Preconditions:** A project in inches (UAT-PRJ-03) with a toolpath.
- **Steps:**
  1. On the **Heights** tab type `0.25` in the Retract offset box. Then type `6mm`.
- **Expected:** `0.25` is a quarter inch. `6mm` is 6 millimetres, and the box shows the same length in inches (0.2362). The project file stores 6.35 and 6.
- [ ] Pass

### UAT-TP-29 Browser: rename

- **Preconditions:** A Setup with a toolpath.
- **Steps:**
  1. Click a toolpath. Press **F2**. Type `Rough pocket`, press Enter.
  2. Double-click the Setup row, type a name and press Esc.
  3. Right-click the toolpath, choose **Rename**, clear the box and press Enter.
- **Expected:** Step 1 renames it. In step 2 Esc keeps the old name. In step 3 an empty name is not accepted and the old name stays. Each rename is one undo step.
- [ ] Pass

### UAT-TP-30 Browser: delete

- **Preconditions:** A Setup with two toolpaths.
- **Steps:**
  1. Click a toolpath and press **Del**.
  2. Click **Undo**.
  3. Select the Setup, press **Del**, then click **Undo**.
- **Expected:** The toolpath goes, and Undo brings it back in its place. Deleting the Setup removes its toolpaths too, and Undo brings back all of them.
- [ ] Pass

### UAT-TP-31 Browser: reorder

- **Preconditions:** A Setup with three toolpaths A, B, C, and a second Setup.
- **Steps:**
  1. Right-click B and choose **Move up**. Then **Move down** twice.
  2. Drag C onto A.
  3. Drag A onto the second Setup row.
  4. Right-click the first toolpath: look at **Move up**.
- **Expected:** Move up and down change the order, and the calculation order follows. Dropping C on A puts C where A was. Dropping A on the second Setup moves it to the end of that Setup. **Move up** is greyed out on the first row. Each move is one undo step.
- [ ] Pass

### UAT-TP-32 Browser: disable, enable and duplicate

- **Preconditions:** A calculated toolpath.
- **Steps:**
  1. Right-click it and choose **Disable**. Click **Calculate all**.
  2. Right-click it and choose **Enable**.
  3. Right-click it and choose **Duplicate**.
- **Expected:** A disabled toolpath shows the grey circle with a slash and is skipped by Calculate. Enable brings it back. Duplicate adds "<name> copy" right after it, with the same settings and picks, selected and not calculated. Each is one undo step.
- [ ] Pass

### UAT-TP-33 Re-import keeps the picks

- **Preconditions:** UAT-TP-21 done, saved, with the model file in the project folder.
- **Steps:**
  1. Click **Re-import** in the Model stage.
  2. Select the pocket and open its **Geometry** tab.
- **Expected:** The status bar says "Re-imported 1 models, 0 selections not found". The picked face is still listed. The toolpath still calculates.
- [ ] Pass
