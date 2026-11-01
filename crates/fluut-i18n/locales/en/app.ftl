# SPDX-License-Identifier: MIT OR Apache-2.0
# Application text (D13 section 9). One message per line.
app-title = FluutCAM
app-untitled = No project

# Ribbon
ribbon-stage-project = Project
ribbon-open = Open
ribbon-open-path = Project file
ribbon-open-hint = path/to/project.fluut
ribbon-calculate = Calculate
ribbon-calculate-all = Calculate all
ribbon-post = Post
ribbon-post-processor = Post processor
ribbon-post-output = Output file
ribbon-cancel = Cancel
ribbon-group-file = File
ribbon-group-toolpaths = Toolpaths
ribbon-group-output = Output

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
