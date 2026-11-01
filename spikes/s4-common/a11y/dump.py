# SPDX-License-Identifier: MIT OR Apache-2.0
"""Print the AT-SPI tree of the running application whose name contains argv[1],
then a count of roles and of nodes without a name."""
import sys
from collections import Counter

import pyatspi

want = sys.argv[1]
desk = pyatspi.Registry.getDesktop(0)
apps = [desk.getChildAtIndex(i) for i in range(desk.childCount)]
roles, unnamed = Counter(), Counter()
CONTAINERS = {"filler", "panel", "frame", "window", "application", "group", "section"}


def walk(node, depth):
    if node is None or depth > 40:
        return
    role = node.getRoleName()
    roles[role] += 1
    if not node.name and role not in CONTAINERS:
        unnamed[role] += 1
    if role != "label" or node.name:
        print("  " * depth + f"{role}: {node.name!r}")
    for i in range(node.childCount):
        walk(node.getChildAtIndex(i), depth + 1)


for app in apps:
    if app and want in (app.name or "").lower():
        walk(app, 0)
print("roles:", dict(roles))
print("unnamed:", dict(unnamed))
