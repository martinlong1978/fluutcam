+ Hand-written test post for FluutCAM's Vectric importer; not a vendor
+ file. An imaginary controller in inches that wants a decimal comma, a
+ sign on every coordinate, zero-padded line and tool numbers, semicolon
+ comments, radius arcs and a two-step first rapid.

POST_NAME = "FluutCAM test, comma decimals (inch)"
FILE_EXTENSION = "cnc"
UNITS = "INCHES"
LINE_ENDING = "[13][10]"
LINE_NUMBER_START = 1
LINE_NUMBER_INCREMENT = 1
LINE_NUMBER_MAXIMUM = 9999

VAR LN = [N|A|N|04.0]
VAR TOOLNO = [T|A|T|02.0]
VAR RPM = [S|A|S|1.0]
VAR FEED = [F|C|F|1.1]
VAR XPOS = [X|C|X|+1,4]
VAR YPOS = [Y|C|Y|+1,4]
VAR ZPOS = [Z|C|Z|+1,4]
VAR RAD = [Radius|A|R|1,4]

begin HEADER
"[N];[TOOLNAME]"
"[N]G20 G90"
"[N][T] M6"
"[N][S] M3"

begin TOOLCHANGE
"[N]M5"
"[N][T] M6"
"[N][S] M3"

begin INITIAL_RAPID_MOVE
"[N]G0 [X] [Y]"
"[N]G0 [Z]"

begin RAPID_MOVE
"[N]G0 [X] [Y] [Z]"

begin FIRST_FEED_MOVE
"[N]G1 [X] [Y] [Z] [F]"

begin FEED_MOVE
"[N]G1 [X] [Y] [Z]"

begin FIRST_CW_ARC_MOVE
"[N]G2 [X] [Y] [Radius] [F]"

begin CW_ARC_MOVE
"[N]G2 [X] [Y] [Radius]"

begin FIRST_CCW_ARC_MOVE
"[N]G3 [X] [Y] [Radius] [F]"

begin CCW_ARC_MOVE
"[N]G3 [X] [Y] [Radius]"

begin FOOTER
"[N]M5"
"[N]M2"
