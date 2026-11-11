# Add a Setup

A **Setup** is one way of holding the part on the machine. It says which bodies are machined, how the part is oriented, what the stock is, where the work zero is, and which machine and material to use. The toolpaths belong to a Setup. A part cut in one clamping needs one Setup; a part you flip, or turn on a rotary axis, needs one for each way it is held.

This page shows how to add the three kinds of Setup. [Setups, stock and work zero](Setups-Stock-and-Zero) explains the details.

## The three kinds

| Kind | Use it for | Machine needs |
|---|---|---|
| **3-axis** (`three_axis`) | Ordinary milling: the tool moves in X, Y and Z, and the part does not turn | A normal 3-axis machine |
| **Rotary** (`rotary`) | Cutting while a rotary axis turns the part, for example a shaft or a relief wrapped round a cylinder | A machine with a rotary axis (A) |
| **Indexed** (`indexed`) | Cutting one side at a time: the rotary axis turns the part to an angle, locks, and the cut is 3-axis | A machine with a rotary axis (A) |

## Add one

1. Import a model first. A project with no model cannot have a rotary or indexed Setup, because their stock is sized from the model. (A 3-axis Setup can be added without a model.)
2. Open the **Setup** stage of the ribbon and click **New 3-axis Setup**, **New Rotary Setup** or **New Indexed Setup**.

![The Setup stage of the ribbon](images/ribbon-setup.png)

The new Setup is named "Setup 1", "Setup 2" and so on, added at the end of the list, and selected. It is one undo step. The viewport shows its stock outline and work zero, and the inspector shows what FluutCAM chose.

## What each kind starts with

All three start with the material **MDF**, because the built-in starter tools have cutting data for MDF. The work offset is G54.

**3-axis.** The machine is `fluut/generic-3axis-router@1`. The stock is a block: the model plus 2 mm on each side in X and Y, and 1 mm on top. The work zero is the top front-left corner of the stock.

![A 3-axis Setup](images/setup-added.png)

**Rotary and indexed.** The machine is `fluut/generic-4axis-router@1`, a router with a rotary axis along X. The stock is a cylinder around the X axis through the model's origin, 1 mm bigger in radius than the model and 2 mm longer. The work zero is on the axis at the left end of the stock. The clearance is a cylinder 10 mm outside the stock.

FluutCAM assumes your part lies along the X axis through the model origin. If it does not, set the rotary axis in the **Orientation** group of the inspector (see below).

## Based on another Setup

A Setup can be **based on** an earlier one. It inherits that Setup's machine, material, orientation, bodies and work zero, and it starts from the material the earlier Setup left behind. Use it to flip or turn a part and carry on, and to machine the second side from the stock the first side left.

1. Select the Setup to build on, in the Browser.
2. In the **Setup** stage click **New Setup based on selected**. The new Setup has the same kind as the one you selected.

The new Setup stores only what differs: the parent and the stock (`from_previous`). Changes to the parent flow through to it. The toolpaths are **not** inherited: each Setup has its own list.

The button makes a Setup of the selected Setup's kind. (A 3-axis Setup cannot be based on a rotary or indexed one, or the other way round, because the orientation and zero could not carry over; a project file that does this is refused with "a ... Setup cannot be based on the ... Setup".)

The index angle of an indexed Setup (for example 0 and then 180 degrees for two sides) is a box in its inspector. The reference project `flipside` is a worked example.

## Edit a Setup

Select the Setup in the Browser. The inspector lists its values in boxes and lists.

![The Setup inspector](images/setup-inspector.png)

The **Bodies** and **Orientation** groups sit between the Setup values and the stock:

![The Bodies and Orientation groups](images/setup-bodies-orientation.png)

| Value | How you change it |
|---|---|
| **Name** | Type in the box and press Enter. A Setup needs a name. |
| **Kind** | Choose 3-axis, Rotary or Indexed from the list. See "Change the kind" below. |
| **Material** | Choose one from the list. It picks the cutting data of the tools. |
| **Machine** | Choose one from the machine list. |
| **Work offset** | A list, G54 to G59. |
| **Stock** | Choose the type, then type its sizes. A 3-axis Setup offers a block by margin or a block of a fixed size. A rotary or indexed Setup offers a cylinder. A Setup based on another also offers "From the previous Setup". Every kind also offers "Body" when the model has a body (see below). |
| **Bodies** | Choose which bodies of the model this Setup machines. See "Choose the bodies" below. |
| **Orientation** | Choose the tool axis and X axis, or the rotary axis. See "Set the orientation" below. |
| **Work zero** | Choose the location: a corner or the centre of the stock, or a point on the model (X, Y, Z). A rotary or indexed Setup offers the left or right end of the stock, or a position along the axis. |
| **Clearance** | The safe height above the stock (3-axis), or the radius of the safe cylinder (rotary and indexed). |
| **Index angle** | For an indexed Setup. |

The **Stock** and **Work zero** buttons in the Setup stage select the Setup and mark those rows of the inspector. Number boxes take expressions and units, as in the toolpath inspector (`1/4in`).

**Inherited values.** A Setup based on another takes some values from its parent. The inspector shows such a value with "Inherited from" and the parent's name under it. Change the value and the Setup keeps its own. Click **Reset to inherited** to take the parent's value again.

Every change is one undo step. If the model refuses a change, the status bar says "Not changed" and gives the reason. [Setups, stock and work zero](Setups-Stock-and-Zero) explains the values.

You can also rename, reorder, and delete a Setup in the Browser: see the [Interface tour](Interface-Tour).

## Change the kind

Use the **Kind** list in the inspector to change a Setup from one kind to another.

- **Rotary and Indexed** share the same values. Changing between them writes only the kind.
- **3-axis to Rotary or Indexed, or back,** changes more, because the two need different things. FluutCAM sets the default orientation and work zero of the new kind, a stock that fits it (a cylinder, or a block with the default margins), and the default clearance. A stock that is "From the previous Setup" stays as it is. All of this is one undo step.
- A rotary or indexed kind needs a machine with a rotary axis. If the Setup's machine has none, the change is refused and the status bar names the machine. Choose a machine with a rotary axis first.

The toolpaths stay where they are. If the new kind does not allow one of them, it shows its usual error in the Messages tab. Delete it or move it to another Setup.

## Choose the bodies

The **Bodies** group lists every body of the model with a check box. "All bodies" is the default: the Setup machines everything. Clear it, or clear a body, to machine only some bodies. At least one body must stay checked.

Click **Pick** to choose on the model instead. The viewport shows every body, and a click on any face or curve of a body adds or removes that whole body. Esc or **Done** leaves. See the [Interface tour](Interface-Tour).

## Scale and mirror the model

The **Scale and mirror** group, the last in the inspector (scroll down), scales and flips the model for this Setup, about the model's origin. Type a factor in **Scale, all axes** (for example `25.4` for a part drawn in inches, or `0.5` for half size), and tick **Mirror X**, **Mirror Y** or **Mirror Z** to flip the model across that axis. The stock, the work zero and the handles follow the scaled model. Each change is one undo step. A factor of 0 is refused. A solid from a STEP file cannot be scaled by different factors in different directions: the Setup says so. Setting the factor back to 1 and clearing the mirrors removes the setting.

## Use a body as the stock

Choose **Body** in the **Type** list under **Stock**. Use this when the raw material is a body of your model, such as a casting. The row below picks the body from a list, or with **Pick** on the model.

The stock body is never machined. A new body stock starts with a body this Setup does not machine. If there is none, it starts with the first body.

- In a **3-axis** Setup the stock is the box of the body.
- In an **indexed** Setup the stock is the box of the body too, as it sits at A = 0.
- In a **rotary** Setup the stock is the **bounding cylinder** of the body about the rotary axis: as long as the body along the axis, and as wide as its farthest point from the axis. A body that is not round gives round stock that holds it, so there is more material than the body has. If you want exact round stock, choose Cylinder and type the radius and length.

## Set the orientation

The **Orientation** group says how the model sits on the machine.

- A **3-axis** Setup has **Tool axis (up)** and **X axis**. The tool axis is the direction the tool points down from. To cut the underside of a part, choose the opposite direction.
- A **Rotary or Indexed** Setup has **Rotary axis** and **A = 0 toward**. The rotary axis is the line the part turns about. "A = 0 toward" is the model direction that points up when the axis is at A = 0.

Each one is a list of the six model directions (such as "+Z of the model"). Click **Pick** next to it to take a direction from the model instead: click a flat face, a round face (its axis) or an edge. Esc or **Done** leaves.

The viewport follows your choice. If the choice makes no sense, for example both axes are the same line, or an edge is curved, the Setup shows the reason as a finding. Each change is one undo step.

## Drag handles on the model

Select a Setup and the viewport draws handles on it. Drag one and let go. Nothing changes until you let go, and the change is one undo step. You can still type exact numbers in the inspector.

![The work zero ring and the margin handles](images/setup-handles.png)

- **Work zero.** A ring at the work zero. While you drag it, small circles mark where it can land: the corners and the top centre of the stock, or the two ends of the stock on the axis for a rotary Setup. Near the pointer, more circles show the vertices of the model and the centres of its arcs and circles (not on STL meshes). The nearest one grows. A model point gives a work zero **Point on the model** in a 3-axis Setup; in a rotary or indexed Setup only its position along the axis counts, since the zero stays on the axis. Let go on it and the zero goes there. Let go away from every point and nothing changes.
- **Block margins.** An orange square on the +X side, the +Y side and the top of block stock that is set by margins. Drag one out and the margin grows, in steps of 0.1 mm. The new value shows beside the square.
- **Block size.** The same three squares on a block of a fixed size change **Size X**, **Size Y** and **Size Z**. The block is centred on the model in X and Y, so dragging the +X or +Y square 3 mm grows the size by 6 mm (both sides move); the top square changes the height by what you drag.
- **Cylinder stock.** In a rotary or indexed Setup with cylinder stock, a square on top of the cylinder changes the **Radius**, and a square at its right end changes the **Length**. The cylinder is centred on the model, so both ends move: dragging the end 4 mm changes the length by 8 mm.

Handles hide while you pick on the model.

## What you cannot do in the window yet

Not yet: a dial for the index angle. See [Known limits](Known-Limits).

## Next

[Add a toolpath](Add-a-Toolpath)
