# Save, undo and recover

## What a project is on disk

A project is a folder with a **project file** (`name.fluut`) in it. The file is plain text (TOML), so you can read it, edit it and keep it in version control. Beside it you find:

- the model files you imported, copied into the folder (see [Import a model](Import-a-Model));
- `name.fluut.bak`, the previous saved version, kept every time you save;
- optionally, folders with your own definitions: `tools/`, `machines/`, `posts/` and `templates/` (see [Machines and posts](Machines-and-Posts) and [Tools](Tools)).

Keep the folder together. To move or share a project, move or zip the whole folder.

## Save

**File, Save** (**Ctrl+S**) writes the project file. The title bar shows `*` after the name while there are changes you have not saved, and the star goes away when you save. If you undo back to the saved state, the star goes away too.

FluutCAM saves safely: it writes a new file first, keeps the old file as `name.fluut.bak`, and only then puts the new file in place. A power cut during a save leaves the old file or the new one, not half of each. Saving a project that has not changed gives exactly the same file.

**File, Save as...** (**Ctrl+Shift+S**) saves under a new name or in a new folder. When the folder is new, FluutCAM copies what the project needs next to it: the model files and the `tools/`, `machines/`, `posts/` and `templates/` folders. The new project opens just as the old one did.

A new project (**File, New project...**) is saved at once, empty, so the file exists from the start.

![The saved project](images/saved.png)

## Unsaved changes

If you choose **File, New project...** or **File, Open...** while the project has unsaved changes, a window asks "The project has changes that are not saved. Discard them?" Choose **Discard** to go on, or **Keep editing** to stay.

Two places do not ask yet:

- the **Open** button in the ribbon, which opens the path typed in its box;
- closing the window.

Save before you use them.

## Undo and redo

**File, Undo** (**Ctrl+Z**) and **File, Redo** (**Ctrl+Y**, or **Ctrl+Shift+Z**) step back and forward through your changes. On macOS use Cmd in place of Ctrl. The menu names the change: "Undo Add geometry".

Each of these is one undo step:

- importing a model;
- adding a Setup or a toolpath;
- changing a toolpath setting, or resetting it.

Undo restores the project exactly, including the model files in it: an undone import leaves no body behind. Calculating, simulating and posting are not changes to the project, so they are not undo steps; an undone change simply makes the affected toolpaths out of date.

While you type in a number box, **Ctrl+Z** edits the text, not the project.

The undo history lives as long as the project is open. It is not saved with the project, and opening the project again starts a new history.

## Recover

FluutCAM has the machinery to keep an edit journal (`name.fluut-autosave`) and to replay it after a crash, but **it is not switched on in the window or the command line yet**. Today:

- **Save often.** Anything you have not saved is lost if the program or the computer stops.
- If a save goes wrong, the previous version is in `name.fluut.bak`. Rename it to `name.fluut` to go back.
- Crash reports are written to a folder on your computer and sent nowhere: `%LOCALAPPDATA%\FluutCAM\crashes` on Windows, `~/Library/Application Support/FluutCAM/crashes` on macOS, `~/.local/share/fluutcam/crashes` on Linux. If FluutCAM crashes, please look at the report and send it with a description if you are willing to; it holds the error, the version and your operating system, and no project data.

See [Known limits](Known-Limits).

## Next

[Setups, stock and work zero](Setups-Stock-and-Zero)
