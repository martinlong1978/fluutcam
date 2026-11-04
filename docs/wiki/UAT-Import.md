# UAT: Import

Tests of importing model files, the copy rules, units and bad files. Part of the [user acceptance tests](UAT). Test files: `crates/fluut-cli/tests/projects/block/block.step`, `plate/plate.dxf`, `vcarve/vcarve.dxf`, `fuzz/seeds/stl/tetra.stl`, and two files you make yourself (below). Each test starts from a new project, `my-part`, in the work folder, as in [UAT-PRJ-02](UAT-Project-Files), unless it says otherwise.

**Files to make:**

- `broken.step`: a text file containing `this is not a STEP file`.
- `notes.txt`: any text file.

---

### UAT-IMP-01 Import a STEP file

- **Preconditions:** A new, empty project.
- **Steps:**
  1. Choose **File, Import model...**.
  2. Pick `block.step`.
- **Expected:** The status bar says "Imported block.step (1 bodies)". The Browser lists the body `pocket_block` under **Model**. The viewport shows the block with its pocket. The File menu now offers "Undo Add geometry". The title shows `*`.

![A model imported](images/model-imported.png)

- [ ] Pass

### UAT-IMP-02 The file is copied into the project folder

- **Preconditions:** UAT-IMP-01 done, with `block.step` picked from outside the work folder.
- **Steps:**
  1. Look in the work folder.
  2. Save the project and open `my-part.fluut` in a text editor.
- **Expected:** The folder holds `block.step` (a copy). The project file has a `[[geometry]]` table with `path = "block.step"` (a path with no folder, relative to the project) and `kind = "step"`.
- [ ] Pass

### UAT-IMP-03 A file already in the project folder is not copied

- **Preconditions:** A new project in a folder that already contains `block.step`.
- **Steps:**
  1. Import that `block.step`.
  2. Look in the folder.
- **Expected:** The import works and no `block-2.step` appears. The path in the project is `block.step`.
- [ ] Pass

### UAT-IMP-04 Same name, different file

- **Preconditions:** A project folder that contains a different STEP file named `block.step` (for example a copy of `boss.step` renamed).
- **Steps:**
  1. Import `block/block.step` from the reference projects.
  2. Look in the folder.
- **Expected:** The import works. The folder holds the old `block.step` and a new `block-2.step`. The project refers to `block-2.step`.
- [ ] Pass

### UAT-IMP-05 Import the same file twice

- **Preconditions:** UAT-IMP-01 done (the project holds `block.step`).
- **Steps:**
  1. Import the same `block.step` again.
- **Expected:** FluutCAM does not make a second copy of the identical file. Either the second import works and uses the same file, or it is refused with a message that the body ids clash (`model.import.body_ids`). Nothing breaks and no half-imported state is left. Record which happened.
- [ ] Pass

### UAT-IMP-06 A file that is not STEP

- **Preconditions:** A new project; `broken.step` made as above.
- **Steps:**
  1. Import `broken.step`.
- **Expected:** The status bar turns red: "Failed:" and a message that says the geometry is not a valid STEP file, with a position. The Browser still lists no model. No `broken.step` is left in the project folder. The File menu's Undo is unchanged (nothing was added).

![A failed import](images/import-failed.png)

- [ ] Pass

### UAT-IMP-07 A file with an unknown extension

- **Preconditions:** A new project; `notes.txt`.
- **Steps:**
  1. Choose **File, Import model...**. Check what the dialog offers.
  2. If the dialog lets you choose "all files", pick `notes.txt`.
- **Expected:** The dialog offers STEP, STL, DXF and SVG files. If `notes.txt` can be picked, the import fails with a message that it is not a STEP, STL, DXF or SVG file, and changes nothing.
- [ ] Pass

### UAT-IMP-08 An STL file

- **Preconditions:** A new project; `fuzz/seeds/stl/tetra.stl`.
- **Steps:**
  1. Import `tetra.stl`.
- **Expected:** The import fails: "Failed:" and a message that the file gave no geometry. No model appears and the project folder has no copy of the file. (This is a known limit: see [Known limits](Known-Limits). If the import succeeds, record it: a feature has changed and this test needs updating.)
- [ ] Pass

### UAT-IMP-09 A DXF file

- **Preconditions:** A new project in millimetres; `plate/plate.dxf`.
- **Steps:**
  1. Import `plate.dxf`.
- **Expected:** The import works. FluutCAM warns that the file carries no reliable unit and that it assumed millimetres, the project unit (`model.import.unit_assumed`), in the Messages tab or the status bar. The viewport shows the plate's outline as flat curves. A body is listed under **Model**.
- [ ] Pass

### UAT-IMP-10 The unit follows the project

- **Preconditions:** A new project in inches.
- **Steps:**
  1. Import `plate.dxf`.
  2. Compare the size of the outline in the viewport with UAT-IMP-09.
- **Expected:** FluutCAM warns that it assumed inches. The outline is 25.4 times larger than in UAT-IMP-09, because the same numbers are read as inches.
- [ ] Pass

### UAT-IMP-11 Two DXF files with the same body ids

- **Preconditions:** A new project; `plate.dxf` already imported.
- **Steps:**
  1. Import `vcarve/vcarve.dxf` as well.
- **Expected:** The second import is refused: "Failed:" and a message that a body has the id of a body of the first file (`model.import.body_ids`). The first file stays; the project is unchanged by the failed import. (A known limit.)
- [ ] Pass

### UAT-IMP-12 Undo an import

- **Preconditions:** UAT-IMP-01 done.
- **Steps:**
  1. Choose **File, Undo** (Ctrl+Z).
- **Expected:** The model disappears from the Browser and the viewport. The File menu offers Redo. Redo brings the model back.
- [ ] Pass

### UAT-IMP-13 Import needs a project

- **Preconditions:** FluutCAM started with no project.
- **Steps:**
  1. Open the **File** menu. Press Ctrl+I.
- **Expected:** **Import model...** is greyed out, and Ctrl+I opens no dialog.
- [ ] Pass

### UAT-IMP-14 Cancel the import dialog

- **Preconditions:** A project is open.
- **Steps:**
  1. Choose **File, Import model...** and cancel the dialog.
- **Expected:** Nothing changes: no new model, no new undo step, the title still shows no `*` if there were no changes.
- [ ] Pass

### UAT-IMP-15 A project with a model and no Setup is drawn

- **Preconditions:** UAT-IMP-01 done; no Setup added.
- **Steps:**
  1. Drag in the viewport to orbit. Scroll to zoom. Double-click to fit.
- **Expected:** The model turns, zooms and fits. The Browser shows only the Model. The inspector says "Select a Setup or a toolpath."
- [ ] Pass
