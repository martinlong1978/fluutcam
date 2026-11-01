# SPDX-License-Identifier: MIT OR Apache-2.0
# Validation messages that carry no values (D15 section 1.7). The id is the
# diagnostic code with `.` replaced by `-`. Messages with values stay on the
# English text that fluut-model writes, until diagnostics carry arguments.
param-tool-diameter = the tool diameter must be above 0
param-geometry-drive-empty = select drive curves or drive faces
param-geometry-rest_from-unsupported = rest machining from previous toolpaths is not available yet
param-heights-reference-empty = a height refers to the selection, but the drive selection is empty
param-tool_params-coolant-unsupported = the post has no coolant_on block, so coolant is not switched on
param-linking-ramp-plunge_tool = the tool is not centre cutting and should not plunge
param-contour_2d-depths-both = set either step_down or depth_count, not both
param-contour_2d-compensation-on = controller compensation needs a side other than `on`
param-contour_2d-compensation-post = the post has no cutter_comp blocks
param-contour_2d-tabs-points-empty = tab mode `points` needs at least one point
param-pocket_2d-tool_shape = the tool is not flat or bull nosed: the floor will be scalloped
param-face-tool_shape = the tool is not flat or bull nosed: the face will be scalloped
param-bore-helix-post = the post has no helix support: the helices are posted as lines
param-drill-pitch-required = a tap cycle needs a pitch on the toolpath or on the tool
param-drill-tap-post = the post cannot emit a tap cycle, and taps cannot be expanded
param-trace-depths-both = set either step_down or depth_count, not both
param-trace-curve_depth-unused = curve_depth is ignored with depth_mode = "heights"
param-parallel-stepover_and_scallop = set either stepover or scallop_height, not both
param-parallel-scallop-radius = scallop height needs a tool with a corner radius
param-parallel-slope-range = slope limits must lie in [0, 90]
