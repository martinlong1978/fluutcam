# Project Tool Library References Plan

**Goal:** Let a project refer to tool libraries outside its `tools/` folder, so a library imported from Fusion (or any shared library) can be used by several projects without copying, and the Import tools result window can say "Use in this project" for a library saved anywhere.

**Design:** [D06 §7.2.1](../design/D06-project-model.md) (the file key, paths, order, duplicates, missing files, format version, security); [S 02 §2.1](../spec/02-data-formats.md); [D16 §10.2](../design/D16-tool-library-import.md).

## Decisions

- One top-level key, `tool_libraries = ["...", ...]`, strings as written: relative to the project file with `/`, or absolute. Never URLs or network paths (`project.tools.rejected`).
- Load order in the project layer: `tools/*.toml`, the references in list order, `--tools` files. The existing rule stays: the same library id and version loaded later replaces the earlier one. Replacing one that the project itself loaded earlier is a warning (`project.tools.replaces`), so a shadowed library is visible. The same file twice is loaded once (`project.tools.duplicate`).
- A missing file is a warning (`project.tools.missing`), not an error; the reference is kept.
- Format `fluut.project/1.1` is written only when the list is not empty; every other project is written as before (the golden projects are untouched).
- One edit, `Edit::SetToolLibraries(Vec<String>)`, replaces the list. `Session::apply`, `undo` and `redo` reload the project-layer tool libraries when the list differs from the loaded one, so the app needs no reload of the project and the CLI needs no change.
- Save As into another folder rewrites relative references to name the same files; the libraries are shared, so they are not copied.
- GUI: the Library ribbon command **Add tool library…** (native open dialog), a **Tool libraries** list in the browser with Remove and a mark on the ones that did not load (no change to the inspector, which the Setup fixtures branch is editing), and **Use in this project** in the Import tools result window. A library already in `tools/` only gets its libraries read again (`ReloadToolLibraries`, no undo step).

## Tasks

- [x] **Model.** `Project.tool_libraries`, reader (`opt_strs`), writer (minor 1 only when used), `PROJECT_MINOR = 1`, `Edit::SetToolLibraries`, `Library::load_tool_references`, `load_tools_dir`, `remove_tool_libs`, `check_tool_ref`, `resolve_tool_ref`, `tool_ref_for`, `names_tool_file`. 14 tests in `crates/fluut-model/tests/tool_refs.rs`.
- [x] **Core.** `Session` loads references on open, `reload_tool_libraries`, reload on apply, undo and redo, `--tools` files kept on top, Save As rebasing. 10 tests in `crates/fluut-core/tests/tool_libraries.rs`.
- [x] **App.** `Command::AddToolLibrary` (`library.add_tool_library`, new icon `add-tool-library.svg`), `DialogKind::AddToolLibrary`, `vm::tool_libs` (rows, add and remove edits), the browser list, **Use in this project**, `jobs::Edit::ReloadToolLibraries`. 4 view-model tests, 7 window tests in `ui/tests/tool_libs.rs`, 3 changed or new in `ui/tests/tool_import.rs`; scripted dialogs throughout.
- [x] **CLI.** No code: `fluut calc/sim/post/run` open projects through `Session::open_with`. 5 tests in `crates/fluut-cli/tests/tool_libraries.rs`.
- [x] **Docs.** D06 §7.2.1, S 02, D16 §10.2, wiki `Tools`, `Import-Tools`, `Known-Limits`, `Command-Line`, `Save-Undo-Recover`, `Interface-Tour`, `UAT-Tools` (UAT-TLS-13 to 17), `UAT-Project-Files` (UAT-PRJ-19, 20), diagnostics advice.

## Not done

- Scripts (`fluut-script`) have no function to read or change the list; a script can edit the project file's key through the normal save path only by `Edit` calls that do not exist yet.
- Reordering references in the window (edit the project file).
- A library browser that shows the tools of each library (the Tool list of a toolpath already does).
- A size cap on referenced library files (the same as for `tools/`: none beyond memory).
- Real Windows UNC behaviour was not observed; a network path is rejected by its spelling before any file access.
