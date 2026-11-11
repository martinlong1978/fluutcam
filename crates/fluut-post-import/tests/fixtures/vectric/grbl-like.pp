+ Hand-written test post for FluutCAM's Vectric importer; not a vendor
+ file. A Grbl-like router in millimetres: arcs with incremental centres,
+ a manual tool change as a pause, the feed written on the first move.

POST_NAME = "FluutCAM test, Grbl-like (mm)"
FILE_EXTENSION = "gcode"
UNITS = "MM"
LINE_ENDING = "[10]"

VAR RPM = [S|A|S|1.0]
VAR FEED = [F|C|F|1.1]
VAR XPOS = [X|C|X|1.3]
VAR YPOS = [Y|C|Y|1.3]
VAR ZPOS = [Z|C|Z|1.3]
VAR ICEN = [I|A|I|1.3]
VAR JCEN = [J|A|J|1.3]
VAR ZHOME = [ZH|A|Z|1.3]

begin HEADER
"(Job for [TOOLNAME])"
"G21 G90 G17"
"G0 [ZH]"
"M3 [S]"

begin TOOLCHANGE
"M5"
"G0 [ZH]"
"M0 (Load T[T] [TOOLNAME])"

begin NEW_SEGMENT
"M3 [S]"
"([TOOLPATH_NAME])"

begin RAPID_MOVE
"G0 [X] [Y] [Z]"

begin FIRST_FEED_MOVE
"G1 [X] [Y] [Z] [F]"

begin FEED_MOVE
"G1 [X] [Y] [Z]"

begin FIRST_CW_ARC_MOVE
"G2 [X] [Y] [I] [J] [F]"

begin CW_ARC_MOVE
"G2 [X] [Y] [I] [J]"

begin FIRST_CCW_ARC_MOVE
"G3 [X] [Y] [I] [J] [F]"

begin CCW_ARC_MOVE
"G3 [X] [Y] [I] [J]"

begin FOOTER
"M5"
"G0 [ZH]"
"M30"
