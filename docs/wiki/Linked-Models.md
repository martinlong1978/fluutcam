# Linked models

A **linked model** is a model that stays tied to the design it came from. When the design changes, FluutCAM can update the model and keep your picks: every face and edge a toolpath uses is found again **by its name**, not by where it sits in the file. If a picked face or edge is gone, FluutCAM tells you which one and which toolpath. It never drops the pick without telling you, and that toolpath will not calculate or post until you pick again.

Linked models come from [SolidCraft](https://github.com/tangency-labs/solidcraft) as `.craftx` files (SolidCraft's craft exchange format). SolidCraft names every face from the design's history, so a face keeps its name when you add a hole, move a feature or change a size.

> The `.craftx` format is still a draft, and SolidCraft does not write it yet. FluutCAM reads the draft. Until SolidCraft ships it, you can try linked models with the test files described in [UAT-Import](UAT-Import#linked-models).

## Open a `.craftx` file

There are three ways:

- **Import model** in the Model stage, in an open project. The dialog lists `.craftx` files with the other models, and has a filter for SolidCraft linked models only.
- **From the command line or a file manager:** `fluutcam bracket.craftx`, or `fluutcam --open bracket.craftx`. FluutCAM asks where to save a new project for it (beside the file, named after it), asks for the unit, and imports the file. Each launch opens its own window; a running FluutCAM does not take the file over yet.
- **With a project:** `fluutcam p.fluut bracket.craftx` opens the project and then the file.

A linked file is **never copied** into the project folder. The project refers to the file where it is (by a relative path when they share a folder, else by its full path), so FluutCAM sees the next save of the design. **Save as** to another folder keeps pointing at the same file.

If the open project already links the design in the file you open, FluutCAM does not import it a second time. It asks: **Update from this file**, or **Cancel** (from the command line also **Open in a new project**). Updating from another file makes the project refer to that file from then on.

## The link in the Model panel

Under **Model** in the Browser, each linked model shows:

- **Linked model** and the file name;
- **From** the app and version that wrote the file;
- **Revision** of the design the project holds;
- **Saved**: when the file was written (UTC);
- **Design parameters (read only)**: the sizes the design exposes, such as `wall = 4 mm`. Change them in SolidCraft, not here;
- the **Update** button.

## When the design changes

FluutCAM checks the linked files when its window gains focus and every 2 seconds while it has focus. When a file has changed, the status bar shows **Linked model changed: bracket.craftx** with **Update** and **Later**, and the Model panel says "The file has changed since the project took it." Nothing is re-read or recalculated on its own.

- **Update** reads the file again and finds every pick by its name. It is one undo step: **Undo** brings the old revision back, picks and all.
- **Later** hides the notice until the file changes again. The Update button in the Model panel still works.

After an update, the toolpaths whose model changed are marked stale (amber dot). Calculate them again.

## The update report

Update shows one window, **Model updated**, with a line for every picked face or edge of every toolpath:

| Report says | Outcome | What it means | What to do |
|---|---|---|---|
| **Found** | exact | The face or edge is there under its name. The pick moved with it. | Nothing. |
| **Merged** | merged | The face was merged with another face (for example a rib between two pockets was removed). The pick follows the merged face. | Check the toolpath: it now cuts the bigger face. |
| **Split** | split | The face was cut into pieces (for example a rib now divides a pocket floor). The pick uses **all** the pieces. | Check the toolpath: a pocket over two pieces or a contour around them may not be what you meant. Remove the pieces you do not want. |
| **Check** | by_geometry | SolidCraft could only find the face by its shape, not from the design's history, so the name may sit on the wrong face. The pick is kept. | Look at the face the toolpath uses and confirm it is the right one. |
| **Missing** | missing | Nothing in the new revision answers to the name: the feature was deleted or replaced, or a round took the edge away. The toolpath is **broken**. | Pick again (below). |

Each line names the toolpath and says what happened in plain words. **Show** selects the toolpath. The last line counts the picks and the broken toolpaths. Messages that are not about picks go to the Messages tab.

## A broken toolpath and how to re-pick

A broken toolpath has a red broken-ring icon in the Browser, with the tooltip "Broken: a picked face or edge is not in the model any more". It keeps its last result in the viewport, so you can see what it used to cut, but **Calculate** and **Post** refuse it with the error `model.pick.missing`, which names the face, where it was and the toolpath.

To fix it:

1. Select the toolpath (in the Browser, or with **Show** in the report).
2. At the top of the inspector, the red box **This toolpath is broken** names the missing face or edge and its setting.
3. Click **Pick again**. FluutCAM takes the lost pick out and starts picking that setting on the model.
4. Click the face or edge that the toolpath should use now, then **Done**.

The new pick takes the name of the face you clicked, and the toolpath is no longer broken. A project opened on a design that changed while it was closed shows its broken toolpaths at once, before any update.

## From the command line and scripts

`fluut model update <project> [--model <id>]` does the same update without the window and saves the project; see [Command line](Command-Line#update-a-linked-model). The script function `project.model.reimport(id)` returns the same outcomes ([Scripting](Scripting)).

## See also

- [Import a model](Import-a-Model)
- [Troubleshooting](Troubleshooting#linked-models)
- [Known limits](Known-Limits)
