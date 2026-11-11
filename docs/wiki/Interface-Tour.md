# Interface tour

This page names every part of the FluutCAM window, so the other pages can say "the Browser" or "the status bar" and you know where to look.

![The FluutCAM window with a project open](images/reference-project.png)

## From top to bottom

**Title bar.** The FluutCAM name, then the name of the open project. A `*` after the name means the project has changes you have not saved. On the right:

- a grey **mm** or **inch** badge: the unit the project shows. Click it to switch the project between mm and inch. It changes what the window shows and what you type, and it is one undo step. Inside the project file every length is in millimetres.
- **Light theme** or **Dark theme**: switches between the two.
- **Settings**: see [Settings and updates](Settings-and-Updates).
- **Getting started**: opens or closes the [Getting started card](Getting-Started), a checklist for a first job. It shows at the first start.
- **About FluutCAM**: the version and licence.

**Ribbon.** The tabs under the title bar choose a stage of the work, from left to right: Model, Setup, Toolpaths, Simulate, Post and Library. Each stage shows groups of commands. Hover a button to see its name and its shortcut. A button that does not apply now is greyed out; for example Save is greyed out until a project is open.

![The Model stage of the ribbon with no project open](images/ribbon-model.png)

- **Model**: **New project**, **Open**, **Save**, **Save as**, **Import model** and **Re-import**. Open, Save as and Import open your system's file dialog.
- **Setup**: **New 3-axis Setup**, **New Rotary Setup**, **New Indexed Setup** and **New Setup based on selected**, then **Stock** and **Work zero**. They select the Setup and mark the matching row of the inspector, where you edit it. See [Add a Setup](Add-a-Setup).
- **Toolpaths**: galleries of toolpath types (2D, Holes, 3D, Rotary, Utility) that suit the selected Setup's kind, then **Calculate selected** and **Calculate all**.
- **Simulate**: **Simulate** and the playback buttons (play or pause, step back, step forward).
- **Post**: **Post** and the list of posts.
- **Library**: **Import tools…** turns a Fusion tool library into a FluutCAM one ([Import tools](Import-Tools)). **Add tool library…** lets the open project use a tool library file from any folder ([Tools](Tools)). Tools, Machines and Posts are greyed out ("Coming soon"): browsing and editing the libraries is not built yet.
- **Edit** (on every stage): **Undo** and **Redo**.
- **Job**: **Cancel**, available while a job runs.

**Panels.** Below the ribbon the window has these panels. Drag the lines between them to resize.

| Panel | What it shows |
|---|---|
| **Browser** (left) | A tree of the project: each Setup with its toolpaths, then the **Model** with its bodies, then the **Tool libraries** the project refers to (with **Remove** and **Add library…**). Click a Setup or a toolpath to select it. You can rename, reorder, disable, duplicate and delete rows (below). |
| **Viewport** (middle) | The 3-D view of the selected Setup: the model, the stock outline, the work zero and the calculated toolpaths. |
| **Inspector** (right) | The details of the selection, and where you change toolpath settings. |
| **Messages** (below the viewport) | Warnings and errors from calculating, per toolpath. |
| **Simulation** (a tab beside Messages) | Playback of the cut. |

**Status bar.** The bottom line. It says what the program last did ("Calculated 3 toolpaths"), shows a progress bar and a **Cancel** button while a job runs, and turns red when a job fails. If you have asked for update checks, it also says when a newer version exists.

## Browser

The Browser lists each Setup, with its kind (for example `three_axis`) beside it, and under it the toolpaths in machining order. Each toolpath has a status icon. The shapes differ as well as the colours, so you can tell them apart without colour:

| Icon | Meaning |
|---|---|
| Hollow grey circle | Not calculated yet |
| Ring that fills | Calculating |
| Green filled circle | Calculated |
| Amber ring with a dot | Inputs changed since it was calculated: calculate again |
| Amber triangle outline | Calculated, with warnings |
| Red filled triangle | Cannot be calculated: see Messages |
| Grey circle with a slash | Disabled |

Hover over a toolpath to see its type, tool and status.

**Rename.** Select a row and press **F2**, or double-click the row, or choose **Rename** from its context menu. Type the new name and press Enter. Esc cancels. Leaving the box also saves the name. An empty name is refused.

**Delete.** Select a row and press **Del**, or choose **Delete** from the context menu. Deleting a Setup deletes its toolpaths. If the model refuses, the status bar says why. Undo brings it back.

**Reorder.** Drag a row onto another row. A toolpath dropped on a toolpath takes its place. A toolpath dropped on a Setup goes to the end of that Setup, so this also moves it to another Setup. A Setup dropped on a Setup takes its place. Or right-click a row and choose **Move up** or **Move down**. Toolpaths run in the order listed, and a Setup cannot move above the Setup it is based on. If a move is not allowed, the status bar says "Not changed" and why.

**Disable and enable.** Right-click a toolpath and choose **Disable**. A disabled toolpath shows the grey slashed circle and the calculation skips it. The same menu item says **Enable** to turn it on again.

**Duplicate.** Right-click a toolpath and choose **Duplicate**. The copy is named "<name> copy", goes right after the original and is selected.

**Context menu.** Right-click a row: Rename, Move up, Move down, Duplicate and Disable or Enable (toolpaths only), Delete.

Each of these is one undo step. They need an idle program: Duplicate waits while a job runs.

The **Model** lists the bodies of the imported files. A project with no Setup shows its model in the viewport, so you can see an import straight away. **Tool libraries** below it lists the tool library files the project refers to ([Tools](Tools)); a library that could not be loaded has a warning mark and says why.

## Viewport

| Action | Result |
|---|---|
| Drag | Orbit around the model |
| Right or middle drag | Pan |
| Mouse wheel | Zoom |
| Click | Pick the face or the toolpath under the pointer, and select it. In pick mode, add or remove the face or curve under the pointer |
| Double-click | Zoom to fit |

The small coloured arrows at one corner of the stock are the **work zero**: the red arrow is X, green is Y and blue is Z. Toolpaths are drawn over the model. The selected toolpath is bright orange, and its name shows in the top-left corner of the viewport.

**Pick mode.** When you click **Pick** in a toolpath's Geometry tab, a banner at the top of the viewport says what you are picking ("Picking Drive faces: click to add or remove, Esc or Done to finish"). The face or curve under the pointer lights up, and the picked ones are tinted. Esc or **Done** leaves. See [Add a toolpath](Add-a-Toolpath).

**Body pick mode.** The **Pick** buttons in a Setup's inspector (Bodies, body stock, and the orientation) work the same way. For **Bodies** and body stock the viewport shows every body of the model, and a click on any face or curve toggles its whole body. The chosen bodies are tinted. The pointer lights up only the face or curve under it, not the whole body. For an orientation, a click on a flat face, a round face or an edge fills the value and ends the pick.

**Handles.** When a Setup is selected, the viewport draws drag handles on it: a ring for the work zero, orange squares for the margins or size of block stock and the radius and length of cylinder stock, and for an indexed Setup a ring round the rotary axis with a pointer for the index angle. Drag one and let go to change the Setup in one undo step. Handles hide in pick mode. See [Add a Setup](Add-a-Setup).

If the computer has no usable graphics device, the viewport says so and stays off. Everything else still works.

## Inspector

Select a **Setup** and the inspector lists its name, kind, material, machine, work offset, stock, work zero and clearance, then its toolpath count. You can edit all of these. The **Bodies** and **Orientation** groups have **Pick** buttons for choosing on the model. A value taken from the parent Setup shows "Inherited from" and can be reset. See [Add a Setup](Add-a-Setup) and [Setups, stock and work zero](Setups-Stock-and-Zero).

![The inspector for a toolpath](images/calculated.png)

Select a **toolpath** and the inspector shows its name, its type, its Setup and its tool, then five tabs:

| Tab | What is on it |
|---|---|
| **Tool** | Spindle speed, feeds and coolant |
| **Geometry** | Which faces or curves the toolpath works on |
| **Heights** | The clearance, retract, feed, top and bottom heights |
| **Passes** | How the tool covers the area: stepover, step-down, direction and so on |
| **Linking** | How the tool moves between passes: lead-ins, ramps, retracts |

Each setting shows its value. A setting you have not changed shows its default in small grey text underneath ("Default: 0.6 mm"). A setting you have changed has a **Reset to default** link.

To change a number, click its box and type. You can type an expression, such as `6/2`, `1/4in` or `tool.diameter*0.4`. Press Enter, or click elsewhere, to apply it. If the text is not a value, a red message says so and nothing changes. A choice (such as climb or conventional) is a row of buttons or a list. A yes-or-no setting is a check box.

The **Tool** tab starts with a **Tool** list. Heights are a list and an offset. Faces and curves have a **Pick** button, a count, **Clear**, and a list of the picked items, each with **Remove**. See [Add a toolpath](Add-a-Toolpath).

If a setting has a problem, the message appears right under it, in amber for a warning and red for an error.

## Messages and the Simulation tab

**Messages** lists the diagnostics of every toolpath in the project, and any warnings from opening the project: the kind (Warning or Error), the toolpath, and the text. Hover over a message to see its code. [Diagnostics](Diagnostics) explains each one.

![The Simulation tab](images/simulated.png)

**Simulation** is empty until you simulate a Setup. See [Calculate and simulate](Calculate-and-Simulate).

## Next

[Your first project](Your-First-Project)
