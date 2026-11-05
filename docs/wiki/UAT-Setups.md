# UAT: Setups

Tests of adding Setups and basing one on another. Part of the [user acceptance tests](UAT). Test files: `block/block.step`, `shaft/shaft.step`, and the `flipside` project. Each test starts from a new project (see [UAT-PRJ-02](UAT-Project-Files)) unless it says otherwise.

---

### UAT-SET-01 Add a 3-axis Setup

- **Preconditions:** A new project with `block.step` imported.
- **Steps:**
  1. Open the **Setup** stage and click **New 3-axis Setup**.
- **Expected:** The Browser lists "Setup 1" and selects it. The status bar says "Added Setup 1". The viewport shows a stock outline round the block and the work zero at the top front-left corner of the stock. The inspector shows kind `three_axis`, material MDF, machine `fluut/generic-3axis-router@1` and work offset G54. The stock is larger than the model by 2 mm each side in X and Y and 1 mm on top.

![A 3-axis Setup](images/setup-added.png)

- [ ] Pass

### UAT-SET-02 Add a 3-axis Setup with no model

- **Preconditions:** A new project with no model.
- **Steps:**
  1. Open the **Setup** stage and click **New 3-axis Setup**.
- **Expected:** "Setup 1" is added (the status bar says "Added Setup 1") with kind `three_axis`, material MDF and the 3-axis machine. A 3-axis Setup needs no model. (Rotary and indexed Setups do: see UAT-SET-05.)
- [ ] Pass

### UAT-SET-03 Add a rotary Setup

- **Preconditions:** A new project with `shaft/shaft.step` imported.
- **Steps:**
  1. Open the **Setup** stage and click **New Rotary Setup**.
- **Expected:** "Setup 1" is added. The inspector shows kind `rotary`, machine `fluut/generic-4axis-router@1` and material MDF. The viewport shows a cylinder stock outline round the shaft: its radius is 1 mm larger than the shaft's and it is 2 mm longer. The work zero is on the axis at the left end.
- [ ] Pass

### UAT-SET-04 Add an indexed Setup

- **Preconditions:** A new project with `shaft/shaft.step` imported.
- **Steps:**
  1. Open the **Setup** stage and click **New Indexed Setup**.
- **Expected:** "Setup 1" is added with kind `indexed`, the 4-axis machine and a cylinder stock, as in UAT-SET-03.
- [ ] Pass

### UAT-SET-05 Rotary and indexed need a model

- **Preconditions:** A new project with no model.
- **Steps:**
  1. Open the **Setup** stage and click **New Rotary Setup**.
  2. Repeat with **New Indexed Setup**.
- **Expected:** Each time the status bar turns red: "Failed:" and a message that you must import a model before adding a rotary or indexed Setup. No Setup is added and **Undo** is unchanged.
- [ ] Pass

### UAT-SET-06 A second Setup gets the next number

- **Preconditions:** UAT-SET-01 done.
- **Steps:**
  1. Add another 3-axis Setup.
- **Expected:** "Setup 2" is added at the end of the list and selected. "Setup 1" is unchanged.
- [ ] Pass

### UAT-SET-07 Add a Setup based on another

- **Preconditions:** UAT-SET-01 done, with "Setup 1" selected.
- **Steps:**
  1. Click **New Setup based on selected** in the Setup stage.
- **Expected:** "Setup 2" is added with kind `three_axis` (the kind of the Setup it is based on). The inspector shows it takes the machine, material and bodies of "Setup 1", and that its stock comes from the previous Setup. It has no toolpaths of its own.
- [ ] Pass

### UAT-SET-08 Based-on is not offered without a selection

- **Preconditions:** A project with a Setup, but nothing selected (or a toolpath of no Setup selected).
- **Steps:**
  1. Look at **New Setup based on selected** in the Setup stage.
- **Expected:** It is greyed out when there is no Setup. It works when a Setup is selected (and also when a toolpath of a Setup is, or when a face is picked in it).
- [ ] Pass

### UAT-SET-09 Based on a rotary Setup gives a rotary Setup

- **Preconditions:** A project with `shaft.step` and a rotary "Setup 1", selected.
- **Steps:**
  1. Click **New Setup based on selected**.
- **Expected:** "Setup 2" is added with kind `rotary`. There is no way to ask for a 3-axis Setup based on a rotary one: the button takes the kind of the selected Setup.
- [ ] Pass

### UAT-SET-10 Based on a 3-axis Setup gives a 3-axis Setup

- **Preconditions:** A project with a model and a 3-axis "Setup 1", selected.
- **Steps:**
  1. Click **New Setup based on selected**.
- **Expected:** The new Setup has kind `three_axis`. The window cannot make a rotary Setup based on a 3-axis one (the project file could, and it is refused: `setup.add.kind`).
- [ ] Pass

### UAT-SET-11 An indexed Setup based on an indexed one

- **Preconditions:** A project with `shaft.step` and an indexed "Setup 1" (from **New Indexed Setup**), selected.
- **Steps:**
  1. Click **New Setup based on selected**.
- **Expected:** "Setup 2" is added with kind `indexed`, and its stock comes from the previous Setup.
- [ ] Pass

### UAT-SET-12 Undo and redo adding a Setup

- **Preconditions:** UAT-SET-01 done.
- **Steps:**
  1. Press Ctrl+Z (or click **Undo**), then Ctrl+Y (or **Redo**).
- **Expected:** After Ctrl+Z the Setup disappears from the Browser and the viewport, and **Redo** is available. After Ctrl+Y it returns, with the same name.
- [ ] Pass

### UAT-SET-13 Open a project with indexed Setups

- **Preconditions:** A copy of the `flipside` folder.
- **Steps:**
  1. Open `flipside/flipside.fluut`.
  2. Select each Setup in turn.
- **Expected:** The Browser lists two indexed Setups. The inspector shows their kind and the index angles 0 and 180 degrees. The viewport shows the stock for each. No error appears in the Messages tab.
- [ ] Pass

### UAT-SET-14 Setup values are read only in the window

- **Preconditions:** UAT-SET-01 done.
- **Steps:**
  1. Select "Setup 1". Try to click and type in the kind, material, stock and work zero fields. Then click **Stock** and **Work zero** in the Setup stage.
- **Expected:** The values show but cannot be edited. The two buttons select the Setup and mark the matching row of the inspector; nothing changes in the project. (Editing them is planned: see [Known limits](Known-Limits).) If a value is editable, record it: a feature has landed and this test needs updating.
- [ ] Pass

### UAT-SET-15 Cancel while a Setup is added

- **Preconditions:** A project with a model.
- **Steps:**
  1. Click **New 3-axis Setup**. Watch the status bar.
- **Expected:** Adding a Setup is instant, so there is nothing to cancel. The Setup appears and the status bar returns to ready. (This test confirms the window does not stay busy.)
- [ ] Pass
