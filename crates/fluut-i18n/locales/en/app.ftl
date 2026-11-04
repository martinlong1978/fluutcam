# SPDX-License-Identifier: MIT OR Apache-2.0
# Application text (D13 section 9). One message per line.
app-title = FluutCAM
app-untitled = No project

# Ribbon
ribbon-stage-model = Model
ribbon-stage-setup = Setup
ribbon-stage-toolpaths = Toolpaths
ribbon-stage-simulate = Simulate
ribbon-stage-post = Post
ribbon-stage-library = Library
ribbon-tooltip-shortcut = { $name } ({ $keys })
ribbon-coming = Coming soon: this command is not connected yet.
ribbon-new-project = New project
ribbon-open = Open
ribbon-open-path = Project file
ribbon-open-hint = path/to/project.fluut
ribbon-save = Save
ribbon-save-as = Save as
ribbon-import-model = Import model
ribbon-reimport = Re-import
ribbon-new-setup-3axis = New 3-axis Setup
ribbon-new-setup-rotary = New Rotary Setup
ribbon-new-setup-indexed = New Indexed Setup
ribbon-new-setup-based-on = New Setup based on selected
ribbon-stock = Stock
ribbon-work-zero = Work zero
ribbon-calculate-selected = Calculate selected
ribbon-calculate-all = Calculate all
ribbon-post = Post
ribbon-post-processor = Post processor
ribbon-post-output = Output file
ribbon-cancel = Cancel
ribbon-simulate = Simulate
ribbon-step-back = Step back
ribbon-play-pause = Play or pause
ribbon-step-forward = Step forward
ribbon-library-tools = Tools
ribbon-library-machines = Machines
ribbon-library-posts = Posts
ribbon-group-file = File
ribbon-group-import = Import
ribbon-group-new-setup = New Setup
ribbon-group-workpiece = Workpiece
ribbon-group-calculate = Calculate
ribbon-group-simulation = Simulation
ribbon-group-playback = Playback
ribbon-group-output = Output
ribbon-group-library = Library
ribbon-group-job = Job
ribbon-gallery-2d = 2D
ribbon-gallery-holes = Holes
ribbon-gallery-3d = 3D
ribbon-gallery-rotary = Rotary
ribbon-gallery-utility = Utility

# Toolpath types (04 section 4.13)
toolpath-type-face = Face
toolpath-type-optimised_load_2d = 2D optimised load
toolpath-type-pocket_2d = 2D pocket
toolpath-type-contour_2d = 2D contour
toolpath-type-trace = Trace
toolpath-type-engrave = Engrave
toolpath-type-chamfer_2d = 2D chamfer
toolpath-type-bore = Bore
toolpath-type-drill = Drill
toolpath-type-optimised_load_3d = 3D optimised load
toolpath-type-pocket_3d = 3D pocket
toolpath-type-parallel = Parallel
toolpath-type-contour = Contour
toolpath-type-horizontal = Horizontal
toolpath-type-spiral = Spiral
toolpath-type-scallop = Scallop
toolpath-type-steep_and_shallow = Steep and shallow
toolpath-type-rotary_rough = Rotary rough
toolpath-type-rotary_along_x = Along X
toolpath-type-rotary_around_a = Around A
toolpath-type-rotary_spiral = Rotary spiral
toolpath-type-manual_nc = Manual NC

# Title bar
title-units = Project units
title-theme-dark = Dark theme
title-theme-light = Light theme
title-about = About FluutCAM

# Dock tabs
tab-browser = Browser
tab-viewport = Viewport
tab-inspector = Inspector
tab-messages = Messages
tab-simulation = Simulation
sim-none = Simulate the Setup to see its stock and findings.
sim-play = Play
sim-pause = Pause
sim-time = { $now } / { $total }
sim-show-stock = Show stock
sim-back = Back
sim-forward = Forward
sim-step = Step by
sim-step-move = Move
sim-step-pass = Pass
sim-step-toolpath = Toolpath
sim-colour = Colour
sim-colour-plain = Plain
sim-colour-rest = Rest material
sim-colour-toolpath = By toolpath
sim-legend-rest = Above tolerance
sim-legend-within = Within tolerance
sim-legend-gouge = Gouge
sim-legend-off = No part below
sim-legend-uncut = Not cut
sim-summary = Rays { $res } mm · rest on the part { $rest } mm³, up to { $max } mm
sim-findings = Findings
sim-no-findings = No findings.
sim-finding-gouge = Gouge { $depth } mm: { $name }, segment { $segment }
sim-finding-rapid = Rapid through stock: { $name }, segment { $segment }
sim-finding-holder = Holder touches the stock: { $name }, segment { $segment }
sim-finding-shank = Shank touches the stock: { $name }, segment { $segment }
sim-finding-flute = Cuts above the flute length: { $name }, segment { $segment }

# Browser
browser-model = Model
browser-body = Body { $id }
browser-empty = Open a project to see its Setups and toolpaths.
status-not-calculated = Not calculated
status-calculating = Calculating { $percent } %
status-done = Calculated
status-stale = Inputs changed: recalculate
status-warning = Calculated with warnings
status-error = Cannot be calculated
status-disabled = Disabled

# Inspector
inspector-empty = Select a Setup or a toolpath.
inspector-tool = Tool
inspector-type = Type
inspector-default = Default: { $value }
inspector-default-none = Default: not set
inspector-default-preset = tool preset { $key }
inspector-reset = Reset to default
inspector-reset-short = Reset
inspector-invalid-entry = Not a value: { $text }
inspector-entry-help = Type a value or an expression, e.g. 6/2, 1/4in, tool.diameter*0.4
inspector-read-only = Edit in the project file for now
inspector-yes = Yes
inspector-no = No
inspector-none = None
inspector-items = { $count } selected
group-tool_params = Tool
group-geometry = Geometry
group-heights = Heights
group-passes = Passes
group-linking = Linking
setup-kind = Kind
setup-material = Material
setup-machine = Machine
setup-work-offset = Work offset
setup-stock = Stock
setup-zero = Work zero
setup-bodies = Bodies
setup-bodies-all = All bodies
setup-toolpaths = Toolpaths
setup-default = Default

# Viewport
viewport-label = 3-D viewport
viewport-help = Drag: orbit · right or middle drag: pan · wheel: zoom · click: pick · double-click: fit
viewport-no-gpu = No GPU device: the viewport is off
viewport-picked-face = Face { $id }
viewport-picked-toolpath = Toolpath { $name }
viewport-fit = Zoom to fit

# Jobs
job-opening = Opening { $file }
job-calculating = Calculating { $name } ({ $done } of { $total })
job-posting = Posting with { $post }
job-idle = Ready
job-opened = Opened { $file }
job-calculated = Calculated { $count } toolpaths
job-cancelled = Cancelled
job-posted = Wrote { $files }
job-failed = Failed: { $reason }
job-open-failed = Cannot open the project
job-post-failed = Cannot post the project
job-simulating = Simulating { $setup }
job-simulated = Simulated: { $count } findings

# Messages tab
messages-empty = No messages.
severity-error = Error
severity-warning = Warning

# About
about-version = Version { $version }
about-tagline = Open-source CAM for CNC milling
about-licence = Licensed under the MIT or Apache-2.0 licence, at your option.
about-font = UI font: Noto Sans, SIL Open Font License 1.1.
about-close = Close

# Settings and the update check (spec 06 §6.4)
title-settings = Settings
settings-updates = Updates
settings-auto-check = Check for updates automatically
settings-check-now = Check now
settings-privacy = A check asks github.com for the signed list of releases and sends nothing else. FluutCAM never downloads or installs an update.
update-prompt-title = Check for updates automatically?
update-prompt-text = FluutCAM can ask github.com once a day whether a newer version exists. It sends nothing but that request. You can change this later in Settings.
update-prompt-yes = Yes
update-prompt-no = No
update-checking = Checking for updates…
update-up-to-date = FluutCAM is up to date.
update-available = Version { $version } is available ({ $date }).
update-available-status = Version { $version } is available
update-details = Details
update-notes = Release notes:
update-download = Download page:
update-copy = Copy link
update-download-hint = Open the page in your browser and download the file for your system yourself.
update-no-key = This build has no update key, so it cannot check for updates.
update-failed = The update check failed: { $detail }
