+ Hand-written test post for FluutCAM's Vectric importer; not a vendor
+ file. Wrapped output: Y is wrapped round a rotary axis along X, written
+ as A with the sign reversed.

POST_NAME = "FluutCAM test, wrapped Y to A (mm)"
FILE_EXTENSION = "nc"
UNITS = "MM"
LINE_ENDING = "[10]"
ROTARY_WRAP_Y = "-A"

VAR RPM = [S|A|S|1.0]
VAR FEED = [F|C|F|1.1]
VAR XPOS = [X|C|X|1.3]
VAR YPOS = [Y|C|Y|1.3]
VAR ZPOS = [Z|C|Z|1.3]

begin HEADER
"G21 G90 G17"
"T[T] M6"
"M3 [S]"

begin TOOLCHANGE
"M5"
"T[T] M6"
"M3 [S]"

begin RAPID_MOVE
"G0 [X] [Y] [Z]"

begin FIRST_FEED_MOVE
"G1 [X] [Y] [Z] [F]"

begin FEED_MOVE
"G1 [X] [Y] [Z]"

begin FOOTER
"M5"
"M30"
