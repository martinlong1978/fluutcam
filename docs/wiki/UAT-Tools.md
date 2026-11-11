# UAT: Import tools

Tests of **Import tools…** in the window (UAT-TLS-09 to 14) and of `fluut tools import` (UAT-TLS-01 to 08). Part of the [user acceptance tests](UAT). Test files: you make them from the text below (they are hand-written, not from any product). Work in an empty folder with `fluut` on the path. The window tests use `shop.json` too, and the `block` reference project. See [Import tools](Import-Tools).

**File to make, `shop.json`:**

```json
{
  "version": 22,
  "data": [
    { "type": "flat end mill", "description": "6 mm flat", "unit": "millimeters",
      "geometry": { "DC": 6, "OAL": 60, "LCF": 22, "SFDM": 6, "NOF": 2 },
      "post-process": { "number": 1 },
      "holder": { "segments": [ { "height": 40, "lower-diameter": 20, "upper-diameter": 30 } ] },
      "start-values": { "presets": [ { "name": "MDF", "n": 18000, "v_f": 2000, "v_f_plunge": 600, "stepover": 2.4, "stepdown": 3 } ] } },
    { "type": "ball end mill", "description": "1/4 in ball", "unit": "inches",
      "geometry": { "DC": 0.25, "OAL": 2.5 }, "post-process": { "number": 2 } },
    { "type": "dovetail mill", "description": "Dovetail", "unit": "millimeters", "geometry": { "DC": 10 } }
  ]
}
```

---

### UAT-TLS-01 Import a library

- **Steps:**
  1. Run `fluut tools import shop.json -o shop.tools.toml`. Check the exit status.
- **Expected:** A warning `import.tool.unsupported_type` for tool 3 "Dovetail" and a warning `import.fields.dropped`. The last line says `2 of 3 tools imported as library fusion/shop (1 skipped)`. Exit status 0. The file `shop.tools.toml` exists.
- [ ] Pass

### UAT-TLS-02 What the file holds

- **Preconditions:** UAT-TLS-01 done.
- **Steps:**
  1. Open `shop.tools.toml` in a text editor.
- **Expected:** `id = "fusion/shop"`. A tool `t1-6-mm-flat` with `diameter = 6.0`, `overall_length = 60.0`, `holder = { diameter = 30.0, length = 40.0 }` and a preset `MDF` with `rpm = 18000.0`, `feed = 2000.0`. A tool `t2-1-4-in-ball` with `diameter = 6.35` and `overall_length = 63.5`. No dovetail.
- [ ] Pass

### UAT-TLS-03 Use it in a calculation

- **Preconditions:** UAT-TLS-01 done; a copy of the `block` project.
- **Steps:**
  1. Run `fluut calc block.fluut --tools shop.tools.toml`.
- **Expected:** The same result as without the extra file (the project does not use the new library). No error about the tools file. Exit status 0.
- [ ] Pass

### UAT-TLS-04 Do not overwrite

- **Preconditions:** UAT-TLS-01 done.
- **Steps:**
  1. Run the same command again.
  2. Run it with `--force` added.
- **Expected:** Step 1: error `import.output.exists`, exit status 1, the file is unchanged. Step 2: the file is replaced, exit status 0.
- [ ] Pass

### UAT-TLS-05 Library id and name

- **Steps:**
  1. Run `fluut tools import shop.json -o other.toml --id shop/fusion --name "Shop tools"`.
  2. Run it with `--id "Bad Id"`.
- **Expected:** Step 1: the file says `id = "shop/fusion"` and `name = "Shop tools"`. Step 2: error `import.library.bad_id`, exit status 1, nothing written.
- [ ] Pass

### UAT-TLS-06 A file that is not a tool library

- **Steps:**
  1. Make `bad.json` containing `{ nope`. Run `fluut tools import bad.json -o bad.toml`.
  2. Make `empty.json` containing `{ "hello": 1 }` and import it.
- **Expected:** Both: an error `import.file.invalid`, exit status 1, no output file.
- [ ] Pass

### UAT-TLS-07 Other file types and missing files

- **Steps:**
  1. Make a text file `tools.vtdb` and import it.
  2. Import `missing.json`.
- **Expected:** Step 1: error `import.format.unsupported`, exit status 1. Step 2: error `import.file.unreadable`, exit status 1.
- [ ] Pass

### UAT-TLS-08 Bad command lines

- **Steps:**
  1. Run `fluut tools import shop.json` (no `-o`).
  2. Run `fluut tools export`.
  3. Run `fluut --help`.
- **Expected:** Steps 1 and 2: a message and the usage lines, exit status 2. Step 3: the usage includes the `fluut tools import` line.
- [ ] Pass

---

## In the window

### UAT-TLS-09 Import with the dialogs

- **Preconditions:** `shop.json` made; the window open, with no project.
- **Steps:**
  1. Open the **Library** tab. Check that no field for typing a path is on the ribbon.
  2. Click **Import tools…**. In the first dialog choose `shop.json`.
  3. In the second dialog check the offered name, and save as `shop-gui.toml`.
- **Expected:** Step 1: **Import tools…** is enabled (Tools, Machines and Posts are greyed out). Step 3: the name offered is `shop.toml`. Then a window **Import tools** says `Wrote 2 of 3 tools to` the path, `Library id: fusion/shop`, `Tools read: 3`, `Tools written: 2`, `Tools skipped: 1`, and a count of source keys not used. The warnings include `import.tool.unsupported_type` for tool 3 "Dovetail". It says to open a project to use the library. The file exists and has the same text as in UAT-TLS-02.
- [ ] Pass

### UAT-TLS-10 Cancel

- **Steps:**
  1. Click **Import tools…** and cancel the first dialog.
  2. Click it again, choose `shop.json`, and cancel the second dialog.
- **Expected:** After each, no window opens, nothing is written, and **Import tools…** can be used again.
- [ ] Pass

### UAT-TLS-11 Overwrite

- **Preconditions:** UAT-TLS-09 done.
- **Steps:**
  1. Import `shop.json` again and choose `shop-gui.toml` in the second dialog.
  2. If the system dialog asks to replace the file, answer **No**, then choose a new name instead.
  3. Repeat step 1 and answer **Yes** to replacing.
- **Expected:** The system dialog asks before replacing. With **Yes**, the file is replaced and the result window shows the counts again. A file is never replaced without that question.
- [ ] Pass

### UAT-TLS-12 A Vectric file and a bad file

- **Steps:**
  1. Make a text file `tools.vtdb`. Click **Import tools…** and choose it.
  2. Make `bad.json` containing `{ nope`. Import it and choose `bad.toml` in the second dialog.
- **Expected:** Step 1: no second dialog; the window says `Nothing was imported.` with the error `import.format.unsupported`. Step 2: `Nothing was imported.` with an error `import.file.invalid`, and no `bad.toml`.
- [ ] Pass

### UAT-TLS-13 Use the library from the project's tools folder

- **Preconditions:** a copy of the `block` project, open.
- **Steps:**
  1. Import `shop.json`. The second dialog should start in the project's `tools` folder; save as `shop.toml` there.
  2. In the result window click **Use in this project**.
  3. Select a toolpath, open the inspector's Tool tab and open the tool dropdown.
- **Expected:** Step 1: the result window says the library is in the project's tools folder. Step 2: the window says "This project now uses the library." The project is not reloaded and no `*` appears in the title bar. Step 3: the dropdown lists the tools of `fusion/shop` (`t1-6-mm-flat`, `t2-1-4-in-ball`).
- [ ] Pass

### UAT-TLS-14 Use a library from another folder

- **Preconditions:** a copy of the `block` project, open and saved. A folder `shared` beside the project's folder.
- **Steps:**
  1. Import `shop.json` and save the library as `shared/shop.toml`.
  2. In the result window click **Use in this project**.
  3. Look at the browser, under **Tool libraries**, and at the title bar.
  4. Open the tool dropdown of a toolpath.
  5. Press **Ctrl+Z**.
- **Expected:** Step 2: "This project now uses the library." Step 3: the list shows `shop.toml`; the title bar shows `*`. Step 4: the dropdown lists the tools of `fusion/shop`. Step 5: the library leaves the list and the dropdown, and the `*` goes away.
- [ ] Pass

### UAT-TLS-15 Add and remove a tool library

- **Preconditions:** UAT-TLS-14 done, then undone (the project has no references). The library file `shared/shop.toml` still exists.
- **Steps:**
  1. On the Library tab click **Add tool library…**. Choose `shared/shop.toml`.
  2. Click **Add tool library…** again and choose the same file.
  3. Click **Remove** beside the library in the browser.
  4. Press **Ctrl+Z**.
- **Expected:** Step 1: the file dialog starts in the project's folder; the library is listed and loaded. Step 2: the list does not change; a note says the project already lists that library. Step 3: the library leaves the list and the dropdown. Step 4: it comes back. Every change is one undo step. There is no text box for a path anywhere.
- [ ] Pass

### UAT-TLS-16 A library with no project

- **Steps:**
  1. Start FluutCAM with no project. Click **Import tools…** and import `shop.json`.
- **Expected:** The result window says to open a project to use the library, and has no **Use in this project** button. **Add tool library…** on the Library tab is greyed out.
- [ ] Pass

### UAT-TLS-17 The command line uses the project's libraries

- **Preconditions:** the `block` project copied to a folder, its library `tools/shop.toml` moved to a sibling folder `shared/shop.toml`, and the line `tool_libraries = ["../shared/shop.toml"]` added after `units` in `block.fluut` (also change the first line to `format = "fluut.project/1.1"`).
- **Steps:**
  1. Run `fluut calc block.fluut`.
  2. Change the line to `["../shared/gone.toml"]` and run `fluut calc block.fluut` again.
  3. Restore the line and run `fluut calc block.fluut --tools extra.toml`, where `extra.toml` is a library with the id `shop` and a different tool.
- **Expected:** Step 1: three toolpaths calculated. Step 2: a warning `project.tools.missing` naming `gone.toml`, and the toolpaths report that the tool `shop#flat-6` is missing. Step 3: the library from `--tools` replaces the referenced one, so the toolpaths lose their tool as in step 2.
- [ ] Pass
