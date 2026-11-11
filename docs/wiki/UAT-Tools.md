# UAT: Import tools

Tests of `fluut tools import`. Part of the [user acceptance tests](UAT). Test files: you make them from the text below (they are hand-written, not from any product). Work in an empty folder with `fluut` on the path. See [Import tools](Import-Tools).

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
