# SPDX-License-Identifier: MIT OR Apache-2.0
# Project operations: the File menu, dialogs and default names (plan
# 2026-11-04-app-project-ops).

# Default names
name-numbered = { $name } { $n }
setup-name = Setup
setup-kind-three_axis = 3-axis
setup-kind-rotary = Rotary
setup-kind-indexed = Indexed

# Dialogs
dialog-open-title = Open project
dialog-new-title = New project
dialog-save-as-title = Save project as
dialog-import-title = Import model
dialog-filter-project = FluutCAM projects
dialog-filter-model = Models (STEP, STL, DXF, SVG)
dialog-filter-step = STEP files
dialog-post-title = Post to file
dialog-filter-gcode = G-code files

# Unsaved changes
discard-title = Unsaved changes
discard-text = The project has changes that are not saved. Discard them?
discard-yes = Discard
discard-no = Keep editing

# Job strip
job-creating = Creating { $file }
job-saving = Saving { $file }
job-importing = Importing { $file }
job-adding-setup = Adding a Setup
job-adding-toolpath = Adding a toolpath
job-reimporting = Re-importing models
job-undoing = Undoing
job-redoing = Redoing
job-created = Created { $file }
job-saved = Saved { $file }
job-imported = Imported { $file } ({ $count } bodies)
job-setup-added = Added { $name }
job-toolpath-added = Added { $name }
job-reimported = Re-imported { $count } models, { $unmatched } selections not found
job-undone = Undid { $label }
job-redone = Redid { $label }

# Toolpath type labels
toolpath-type-contour_2d = 2D contour
toolpath-type-pocket_2d = 2D pocket
toolpath-type-parallel = Parallel
toolpath-type-face = Face
toolpath-type-trace = Trace
toolpath-type-chamfer_2d = 2D chamfer
toolpath-type-bore = Bore
toolpath-type-drill = Drill
toolpath-type-optimised_load_2d = 2D optimised load
toolpath-type-engrave = Engrave
toolpath-type-contour = Contour
toolpath-type-horizontal = Horizontal
toolpath-type-spiral = Spiral
toolpath-type-scallop = Scallop
toolpath-type-steep_and_shallow = Steep and shallow
toolpath-type-optimised_load_3d = 3D optimised load
toolpath-type-pocket_3d = 3D pocket
toolpath-type-rotary_rough = Rotary rough
toolpath-type-rotary_along_x = Rotary along X
toolpath-type-rotary_around_a = Rotary around A
toolpath-type-rotary_spiral = Rotary spiral
toolpath-type-manual_nc = Manual NC
