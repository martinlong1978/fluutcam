# Import a model

A project holds one or more model files: the parts you want to machine. This page says which files FluutCAM reads, how to import one, and what happens when it goes wrong.

## Which files

| File | Extension | What you get |
|---|---|---|
| STEP | `.step`, `.stp` | Solid bodies with exact faces. This is the format to use. 3-D toolpaths work on it. |
| DXF | `.dxf` | Flat curves (wire bodies), for 2-D work. |
| SVG | `.svg` | Flat curves, for 2-D work. |
| STL | `.stl` | A triangle mesh. Each separate piece (shell) of the mesh becomes one body, and each triangle is a flat face. 3-D toolpaths can machine it. See [Import an STL file](#import-an-stl-file). |

2-D toolpaths need curves or faces picked. In the toolpath's Geometry tab, click **Pick** and click the entities on the model. The curves of a DXF or SVG import can be picked this way. See [Add a toolpath](Add-a-Toolpath).

## Import a file

1. Open a project, or start one with **New project** in the Model stage. **Import model** is greyed out until there is a project.
2. Click **Import model** in the **Model** stage of the ribbon (or press **Ctrl+I**).
3. Pick the file. The dialog offers STEP, STL, DXF and SVG files.

The status bar says "Imported block.step (1 bodies)". The bodies are listed under **Model** in the Browser, and the model is drawn in the viewport even before the project has a Setup.

![A model imported](images/model-imported.png)

The import is one undo step (the status bar says "Undid Add geometry" when you undo it).

## Import an STL file

An STL file is a mesh of triangles. FluutCAM turns every connected piece (shell) of the mesh into one body, listed in the Browser as `b-1`, `b-2` and so on. Each triangle is one flat face. In the viewport only the real creases (where neighbouring triangles bend by more than 30 degrees) and the border edges are drawn, so a smooth mesh does not look like a net.

To import an STL file and machine it:

1. Open a project and click **Import model**. Pick the `.stl` file. The status bar says how many bodies were imported. FluutCAM also warns that STL files have no unit (see below).
2. Click the model in the viewport or the Browser to select a body. A click on the model picks the whole body. Picking a single face picks one triangle.
3. In the **Setup** stage click **New 3-axis Setup**. The stock is sized from the mesh. You can also use a body as the stock, as with a STEP file ([Setups, stock and zero](Setups-Stock-and-Zero)).
4. In the **Toolpaths** stage add a 3-D toolpath, for example **Parallel** or **Optimised load 3D**. With nothing picked it machines every body.
5. Click **Calculate all**, then **Simulate** in the Simulate stage, then **Post** ([Calculate and simulate](Calculate-and-Simulate), [Post G-code](Post-G-code)).

Things to know about meshes:

- **Open or damaged meshes** (holes, or edges shared by more than two triangles) are still imported, with the warning `model.geometry.mesh.open`. Toolpaths follow the triangles, but results near the gaps may be wrong. Close the mesh in your modelling program if you can.
- **A file with no usable triangles** fails with `model.import.empty`. A file that is not a real STL file gives an error. It never crashes the program.
- **The unit.** An STL file has no unit, so FluutCAM uses the project's unit (mm or inch) and warns (`model.import.unit_assumed`). The window has no control to change this yet. If the part is the wrong size, undo the import, start a project in the other unit, and import again.
- **Faces are triangles.** Faces are not grouped, so picking a face picks one triangle only.
- **Re-import** keeps your picks only while the triangles of the file are in the same order as before.
- A mesh is slower to machine than a STEP model (see [Known limits](Known-Limits)). Where you have a choice, STEP gives the best result.

## Where the file goes

A project folder is meant to be self-contained, so you can move or zip it as a whole.

- A file that is already inside the project folder stays where it is. The project refers to it by a path relative to the project file.
- A file from anywhere else is **copied** into the project folder, after FluutCAM has checked that it loads and has bodies. If the folder already has a different file with that name, the copy is named `name-2.step`, `name-3.step` and so on. If it has an identical file, FluutCAM uses that one.

Change the original and the copy does not change. When you open the project again, FluutCAM tells you if a model file has changed on disk since it was imported (`model.geometry.changed`). To read the files again, click **Re-import** in the Model stage. It re-imports every model file of the project (one undo step for each). **Your picks stay.** Faces and curves are stored by their ids, and FluutCAM maps them to the new model. A pick it cannot find again is dropped and counted in the status bar, and its toolpath reports `param.geometry.selection.missing` until you pick it again and the status bar says how many it read and how many selections it could not find again. There is no choice of a single file yet. The [scripting](Scripting) function `model.reimport` does the same for one file.

## Units

STEP files carry their unit. DXF, SVG and STL files do not. For those FluutCAM assumes the unit the project shows (mm or inch) and warns that it did (`model.import.unit_assumed`). If the part looks the wrong size, undo the import, start the project in the other unit, and import again. The window does not ask for the unit yet.

## When it goes wrong

A failed import changes nothing: the project and its folder stay as they were. The status bar turns red and says why.

![A failed import](images/import-failed.png)

| You see | Why | What to do |
|---|---|---|
| "not a valid STEP file" and a position in the file | The file is not STEP, or is damaged | Re-export it from your CAD program |
| "... gave no geometry" (`model.import.empty`) | The file loaded and had nothing a toolpath can use, for example an STL file with no usable triangles | Check the file in another program, or export it again |
| "not a readable STL file" | The file is not an STL file, or is damaged | Export it again from your program |
| "...: not a STEP, STL, DXF or SVG file" | The file extension is not one FluutCAM reads | Check the file |
| "cannot read ..." | The file is missing, locked or unreadable | Check the path and permissions |
| "body `...` of ... has the id of a body of `...`" | Two imported files give a body the same id. Two DXF files do this, because each calls its first body `b-1` | Not supported yet; use one project per file |
| "cannot copy into ..." | The project folder is not writable | Choose a project folder you can write to |

The [Diagnostics](Diagnostics) page lists every message.

## Next

[Add a Setup](Add-a-Setup)
