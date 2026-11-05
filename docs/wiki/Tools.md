# Tools

A **tool library** is a text file that lists cutting tools: their shape, their size, and cutting data for each material. A toolpath names one tool as `library#tool`, for example `fluut/starter#flat-6` (the tool `flat-6` of the library `fluut/starter`).

## The starter tools

FluutCAM has one built-in library, **`fluut/starter`**, so that a new project can calculate at once. It holds five common bits, each with cutting data for **MDF**:

| Tool | Name | Shape | Diameter | Used first by |
|---|---|---|---|---|
| `flat-6` | 6 mm flat end mill | flat | 6 mm | most 2-D types, optimised load, pocket, horizontal, rotary rough |
| `flat-12` | 12 mm flat end mill | flat | 12 mm | Face |
| `ball-6` | 6 mm ball end mill | ball | 6 mm | Parallel, Contour, Spiral, Scallop, Steep and shallow, rotary finishing |
| `vbit-60` | 60 degree V bit | v | 12 mm | 2D chamfer, Engrave |
| `drill-5` | 5 mm drill | drill | 5 mm | Drill |

The cutting data is cautious data for MDF on a small router. It is a starting point, not a recommendation for your machine. Set your own feeds and speeds before you cut.

## What a tool says

A tool has an `id`, a `name`, a tool `number` (the T number on the machine) and a `shape`:

| Shape | Use |
|---|---|
| `flat` | Flat end mill |
| `ball` | Ball end mill |
| `bull` | Bull-nose end mill (a flat end with rounded corners, `corner_radius`) |
| `v` | V bit (`half_angle_deg`, and `tip_diameter` for a flat tip) |
| `taper` | Tapered tool (`half_angle_deg`, and `tip_diameter` for the tip) |
| `drill` | Drill (`point_angle_deg`, 118 degrees if left out) |

Other settings, all in millimetres: `diameter`, `corner_radius`, `flute_length`, `overall_length`, `shank_diameter`, `pitch` (taps), `holder = { diameter = 20, length = 40 }`. `centre_cutting = false` says the tool cannot plunge straight down (FluutCAM then warns about plunge ramps).

**Cutting data** is a `[[tool.preset]]` table per material:

| Key | Unit | Meaning |
|---|---|---|
| `material` | | The name that a Setup's `material` selects (for example `MDF`) |
| `rpm` | rev/min | Spindle speed |
| `feed` | mm/min | Cutting feed |
| `plunge` | mm/min | Plunge feed |
| `stepover` | mm | Sideways step between passes |
| `step_down` | mm | Depth of each level |
| `optimal_load` | mm | Sideways width of cut for the optimised-load types (optional) |

A toolpath takes these as the defaults for its feeds and passes. If the tool has no preset for the Setup's material, the toolpath cannot find its feeds and says so (`param.required`) until you type the values.

## Add your own tools

Make a file `tools/shop.toml` in the project folder (beside the project file). This is the library used by the `block` reference project:

```toml
format = "fluut.tools/1"
id     = "shop"
name   = "My shop"

[[tool]]
id            = "flat-6"
name          = "6 mm flat"
number        = 1
shape         = "flat"
diameter      = 6.0
flute_length  = 25

  [[tool.preset]]
  material  = "MDF"
  rpm       = 18000
  feed      = 2000
  plunge    = 600
  stepover  = 2.4
  step_down = 4.0
```

Then, in the project file, name the tool in a toolpath: `tool = "shop#flat-6"`. The window cannot pick a tool for a toolpath yet, so this is the way to use your own tools today. The full list of tool settings is in the [data format specification](https://github.com/martinlong1978/fluutcam/blob/main/docs/spec/02-data-formats.md).

The command-line programs also take extra tool files with `--tools <file>`, loaded after the project's `tools/` folder. See [Command line](Command-Line).

## Not yet

- Editing the tool library in the window, and picking a tool for a toolpath there.
- Importing tool libraries from other CAM programs.
- A library of your own tools shared by all your projects (copy the file into each project's `tools/` folder).

## Next

[Command line](Command-Line)
