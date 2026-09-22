#!/usr/bin/env python3
"""Run in kitty: python3 /tmp/plexmaton-activity-line/cellsize.py
Asks the terminal for its cell size in pixels and says which odd cell blocks come out square."""
import sys, termios, tty, select, re
fd=sys.stdin.fileno(); old=termios.tcgetattr(fd)
try:
    tty.setraw(fd); sys.stdout.write("\x1b[16t"); sys.stdout.flush(); buf=b""
    while select.select([fd],[],[],0.5)[0]:
        buf+=sys.stdin.buffer.read(1)
        if buf.endswith(b"t"): break
finally:
    termios.tcsetattr(fd, termios.TCSADRAIN, old)
m=re.search(rb"\x1b\[6;(\d+);(\d+)t", buf)
if not m: print("no reply; this terminal does not report cell size"); sys.exit(1)
h,w=int(m.group(1)),int(m.group(2)); print(f"cell {w}×{h} px, width/height {w/h:.3f}")
for rows in (3,5,7):
    best=min((c for c in range(3,40,2)), key=lambda c: abs(c*w-rows*h))
    print(f"  {rows} rows: {best}×{rows} → {best*w}×{rows*h} px, ratio {best*w/(rows*h):.2f}")
