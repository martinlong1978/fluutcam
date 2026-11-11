+ Hand-written test post for FluutCAM's Vectric importer; not a vendor
+ file. A LinuxCNC-like mill: line numbers, four decimals, plunge and
+ dwell blocks, an automatic tool change with a length offset, and
+ parentheses in names kept out of comments. The feed word has no prefix:
+ the lines write the F.

POST_NAME = "FluutCAM test, LinuxCNC-like (mm)"
FILE_EXTENSION = "ngc"
UNITS = "MM"
LINE_ENDING = "[13][10]"
SUBSTITUTE = "([91])[93]"
LINE_NUMBER_START = 100
LINE_NUMBER_INCREMENT = 5
LINE_NUMBER_MAXIMUM = 99995

VAR LN = [N|A|N|1.0]
VAR RPM = [S|A|S|1.0]
VAR FEED = [F|C||1.1]
VAR XPOS = [X|C|X|1.4]
VAR YPOS = [Y|C|Y|1.4]
VAR ZPOS = [Z|C|Z|1.4]
VAR ICEN = [I|A|I|1.4]
VAR JCEN = [J|A|J|1.4]
VAR SAFE = [SAFEZ|A|Z|1.4]
VAR PAUSE = [DWELL|A|P|1.2]

begin HEADER
"%"
"[N] (Post: [POST_NAME])"
"[N] (First tool: [TOOLNAME])"
"[N] G17 G21 G40 G49 G90 G94"
"[N] G64 P0.01"
"[N] T[T] M6"
"[N] G43 H[T]"
"[N] [S] M3"
"[N] G0 [SAFEZ]"

begin TOOLCHANGE
"[N] M5"
"[N] T[T] M6"
"[N] G43 H[T]"

begin NEW_SEGMENT
"[N] ([TOOLPATH_NAME])"
"[N] [S] M3"

begin RAPID_MOVE
"[N] G0 [X] [Y] [Z]"

begin FIRST_FEED_MOVE
"[N] G1 [X] [Y] [Z] F[F]"

begin FEED_MOVE
"[N] G1 [X] [Y] [Z]"

begin PLUNGE_MOVE
"[N] G1 [X] [Y] [Z] F[F]"

begin FIRST_CW_ARC_MOVE
"[N] G2 [X] [Y] [I] [J] F[F]"

begin CW_ARC_MOVE
"[N] G2 [X] [Y] [I] [J]"

begin FIRST_CCW_ARC_MOVE
"[N] G3 [X] [Y] [I] [J] F[F]"

begin CCW_ARC_MOVE
"[N] G3 [X] [Y] [I] [J]"

begin DWELL_MOVE
"[N] G4 [DWELL]"

begin FOOTER
"[N] M5"
"[N] M30"
"%"
