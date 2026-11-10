# Guided First Project Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** A new user who starts FluutCAM with no project is walked through a first job: a new project and its unit, a model, a Setup, a toolpath, calculate, simulate, and G-code written. The guide is a small card that lists the steps, ticks each as the project shows it done, and has a button per step that runs the command the ribbon runs. It can be closed and reopened, and the closing is remembered.

**Architecture:** The split of [D13](../design/D13-ui-architecture.md) stays. `vm::guide` knows the steps, which are done and which can run, and names a `vm::ribbon::Command` for each; it knows nothing of `egui`. `ui::guide` paints the card and runs the commands it is given through `ui::ribbon::run`, the function the ribbon's own buttons use, so dialogs, the unit question, enabling and undo steps are the ribbon's. The closing is one flag in `fluut-core`'s `settings.toml`, kept by `updates::Updates` (the only app module besides `session` that calls `fluut-core`).

**Design:** [D13](../design/D13-ui-architecture.md) §1, §2, §3 (commands), §6; [the ribbon wiring plan](2026-11-05-ribbon-wiring.md); [the app project operations plan](2026-11-04-app-project-ops.md) (native dialogs, scripted dialogs for tests).

## Global Constraints

All earlier Global Constraints apply. In addition:

- The ribbon holds commands only, never parameters; no path text boxes anywhere. Files come from the native dialogs (`dialogs.rs`); tests use `Dialogs::scripted()`.
- The card runs ribbon commands. It adds no operation of its own, so a command that is disabled in the ribbon is disabled in the card.
- Text goes through the catalog (`guide-*` and `title-guide` in `app.ftl`), with English entries.
- Tests avoid hyphenated string literals (the i18n duplicate-id check reads them as catalog ids).
- Nothing is derived from another CAM product.

## Decisions

1. **A card, not a dock tab.** The card is an `egui::Window`: not modal, movable, with a close button, over the lower left of the window where the Browser is empty at first. A dock tab would hide behind the Browser tab, and a modal would stop the user from using the window. It is shown on the start screen and with a project open alike.
2. **Seven steps, in the order of a first job:** start a project, bring in a model, add a Setup, add a toolpath, calculate, simulate, write the G-code. Each has a title, a one-line hint and one button.
3. **Done follows the project.** A step is done when the snapshot shows it: a project is open; geometry exists; a Setup exists; a toolpath exists; every toolpath that is on is calculated (done or warning) and at least one is; a simulation exists; and, because posting leaves no mark in the project, a flag that the guide sets when it sees the Post job end (`Outcome::Posted`) and clears when another project is opened or created. An edit that makes a toolpath stale, or an undo, takes ticks back. The first step not done is the current one (orange ring, orange button).
4. **A button runs a ribbon command.** New project, Import model, New 3-axis Setup, Add toolpath (of the chosen type), Calculate all, Simulate and Post. Run enabling is the guide's own (a project and no running job; a Setup for a toolpath; a toolpath for calculate, simulate and post) and the ribbon's `enabled` must also agree. The Setup step also has **Show stock** and **Show work zero**, which run the ribbon's Stock and Work zero commands: they select the Setup and point the inspector at the group.
5. **Toolpath type and tool.** Two dropdowns in the toolpath step: the type (the gallery types the current Setup's kind allows; Parallel to start, since it needs nothing picked) and the tool (**Default tool**, or any tool of the store). Choosing a tool sets `pending_tool`; when the add job ends and the window takes the created toolpath, it sends the `tool_edit` for it. A failed add clears the pending tool. A 2-D type needs faces or curves picked afterwards; the card says so in the wiki, not in the window.
6. **Closing is remembered.** `Settings` gains `guide.dismissed` (default false; a settings file without it loads). The X in the card and the title bar's **Getting started** button both write it: closed means dismissed, opened means not. So the next start shows what the user did last. `FluutApp::with_updates` (the real window only) sets the card open when not dismissed; a window built for tests starts with the card closed, so no existing test sees it.
7. **Where it is reopened.** A **Getting started** button in the title bar, beside Settings and About. The ribbon has no Help stage and no menu, and the ribbon's rule (commands for the work only) stays.
8. **The tick is an accessible check box.** A painted ring or tick with a screen-reader label ("Done: …" or "To do: …"), so tests and assistive tools read the state without colours.

## Tasks

### Task 1: Plan
- [x] Plan committed.

### Task 2: View model
- [x] Tests first: done states for no project, new, import, Setup, toolpath, calculate, simulate, post; an edit takes the calculate tick back; the commands of the steps; enabling while a job runs; the type and tool choices and their fallbacks; a failed add clears the pending tool.
- [x] `vm::guide` (`GuideStep`, `GuideState`, `GuideVm`).

### Task 3: Card, settings and wiring
- [x] `fluut-core` `Settings.guide.dismissed`; `Updates::guide_dismissed` and `set_guide_dismissed`.
- [x] `ui::guide`, the title bar button, `ribbon::run` shared, the pending tool applied when the toolpath is created.

### Task 4: Window tests
- [x] Walk the whole card on the `boss` STEP file, with scripted dialogs, ending with G-code written and the dialogs asked in order.
- [x] Type and tool choice; the card and the ribbon run the same command.
- [x] Close and reopen, with the choice read back from the settings file and kept by a second window.

### Task 5: Docs
- [x] Wiki page `Getting-Started`; links from Home, the sidebar, Interface tour, Your first project, Settings and updates; Known limits.
- [x] UAT-GDE-01 to 09 in `UAT-Guide`; the area is in `UAT`; UAT-PRJ-01 notes the card.
- [x] Screenshot `getting-started` (card with two steps done); the following shot closes the card.

### Task 6: End to end
- [x] Merge `main`, all six checks, report.

## Done

The card, the view model, the setting and the tests are in. A first job can be done with the card's buttons from an empty window to a G-code file.

## Open issues

- The card adds a 3-axis Setup only. Rotary and indexed Setups, more than one toolpath and picking faces for a 2-D type are done with the ribbon and the inspector.
- The Post tick lasts for the session; the card does not look for files on disk.
- The card has no "do not show again" box apart from its close button, and no language other than English.
- A user who closes the card on the way has no prompt to bring it back; the title bar button does.
