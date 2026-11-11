# SPDX-License-Identifier: MIT OR Apache-2.0
# Project operations: the File menu, dialogs and default names (plan
# 2026-11-04-app-project-ops).

# Default names
name-numbered = { $name } { $n }
name-copy = { $name } copy
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
job-duplicating = Duplicating a toolpath
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

# Tool library import (plan 2026-11-11-tool-import, D16 section 10.2)
dialog-tools-source-title = Import tool library
dialog-tools-target-title = Save the imported tool library as
dialog-filter-tools-fusion = Fusion tool libraries (JSON)
dialog-filter-tools-vectric = Vectric tool databases (VTDB)
dialog-filter-tools-library = FluutCAM tool libraries (TOML)
dialog-tools-add-title = Add tool library
tools-import-title = Import tools
tools-import-working = Importing { $file }...
tools-import-done = Wrote { $written } of { $read } tools to { $path }.
tools-import-library = Library id: { $id }
tools-import-read = Tools read: { $n }
tools-import-written = Tools written: { $n }
tools-import-skipped = Tools skipped: { $n }
tools-import-dropped = Source keys not used: { $keys } ({ $total } values)
tools-import-dropped-list = Keys not used
tools-import-warnings = Warnings
tools-import-no-warnings = No warnings.
tools-import-failed = Nothing was imported.
tools-import-in-project = The library is in this project's tools folder, so it loads whenever the project is opened.
tools-import-elsewhere = The library is outside this project's tools folder. The project can refer to the file where it is.
tools-import-use = Use in this project
tools-import-used = This project now uses the library.
tools-import-no-project = Open a project to use the library.
tools-import-close = Close

# Vectric post import (plan 2026-11-11-vectric-post-import, D10 section 6.1)
dialog-post-source-title = Import post processor
dialog-post-target-title = Save the imported post as
dialog-filter-post-vectric = Vectric post processors (PP)
dialog-filter-post-library = FluutCAM posts (TOML)
posts-import-title = Import post
posts-import-working = Importing { $file }...
posts-import-done = Wrote post { $id } to { $path }.
posts-import-id = Post id: { $id }
posts-import-name = Post name: { $name }
posts-import-settings = Settings mapped: { $n } of { $of }
posts-import-blocks = Blocks mapped: { $n } of { $of }
posts-import-words = Words written: { $n }
posts-import-base = Base files merged: { $n }
posts-import-warning-count = Warnings: { $n }
posts-import-warnings = Warnings
posts-import-no-warnings = No warnings.
posts-import-failed = Nothing was imported.
posts-import-in-project = The post is in this project's posts folder, so the project finds it whenever it is opened.
posts-import-elsewhere = The post is outside this project's posts folder, so FluutCAM does not find it. Save it as a .post.toml file in the posts folder beside the project file to use it.
posts-import-use = Use this post
posts-import-used = Post now uses this post.
posts-import-no-project = Open a project to use the post.
posts-import-close = Close
