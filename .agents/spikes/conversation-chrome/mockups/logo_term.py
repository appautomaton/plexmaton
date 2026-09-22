#!/usr/bin/env python3
"""Run in a kitty tab: python3 /tmp/plexmaton-activity-line/logo_term.py   (Ctrl-C to stop, stops after 30 s)
Measures the terminal cell first and draws the logo at the odd sizes that come out square for 3 and 5 rows."""
import sys, time
RESET="\x1b[0m"; PURPLE=(139,92,246)
DRIFT=[(130,180,240),(139,92,246),(217,70,239),(139,92,246),(130,180,240),(120,210,205)]
LV=[("─","─","│","╭","╮","╰","╯","│"),("━","━","┃","┏","┓","┗","┛","┃"),("▀","▄","▌","▛","▜","▙","▟","▐")]
G=dict(cs="\U000F09DF",cm="\U000F09DE",c="\U000F0765",co="\U000F0766",sr="\U000F14FB",sro="\U000F14FC",s="\U000F0763",so="\U000F0764")
CORE=[G["cs"],G["cm"],G["c"],G["sr"],G["s"],G["sr"],G["c"],G["cm"]]
CH=[("breathe", [0,0,1,1,2,2,1,1], CORE, 220, False),
    ("outline", [0]*8, [G["co"],G["sro"],G["so"],G["sro"],G["co"],G["sro"],G["so"],G["sro"]], 220, False)]
def col(t,period=2500):
    n=len(DRIFT); x=(t/period)%n; i=int(x); k=x-i; a=DRIFT[i]; b=DRIFT[(i+1)%n]
    return tuple(round(a[j]+(b[j]-a[j])*k) for j in range(3))
def fg(c): return f"\x1b[38;2;{c[0]};{c[1]};{c[2]}m"
def rows(w,h,level,core,frame_c,core_c):
    t,b,m,tl,tr,bl,br,r=LV[level]; mid=(h-1)//2; cx=(w-1)//2; out=[fg(frame_c)+tl+t*(w-2)+tr+RESET]
    for i in range(h-2):
        inner=" "*(w-2)
        if i==mid-1: inner=" "*(cx-1)+fg(core_c)+core+fg(frame_c)+" "*(w-2-cx)
        out.append(fg(frame_c)+m+inner+r+RESET)
    out.append(fg(frame_c)+bl+b*(w-2)+br+RESET); return out
def cell_px():
    import termios, tty, select, re
    fd=sys.stdin.fileno(); old=termios.tcgetattr(fd); buf=b""
    try:
        tty.setraw(fd); sys.stdout.write("\x1b[16t"); sys.stdout.flush()
        while select.select([fd],[],[],0.5)[0]:
            buf+=sys.stdin.buffer.read(1)
            if buf.endswith(b"t"): break
    finally: termios.tcsetattr(fd, termios.TCSADRAIN, old)
    m=re.search(rb"\x1b\[6;(\d+);(\d+)t", buf)
    return (int(m.group(2)), int(m.group(1))) if m else None
def square_width(rows, w, h):
    return min(range(3,41,2), key=lambda c: abs(c*w-rows*h))
def main():
    px=cell_px()
    if px: W3,W5=square_width(3,*px),square_width(5,*px); print(f"cell {px[0]}×{px[1]} px → square blocks: {W3}×3, {W5}×5")
    else: W3,W5=7,11; print("terminal did not report its cell size; assuming JetBrains Mono proportions: 7×3, 11×5")
    sizes=[(W3,3),(W5,5)]
    W,H=W5,5; sys.stdout.write("\x1b[?25l"); t0=time.time()
    try:
        while time.time()-t0<30:
            t=(time.time()-t0)*1000; lines=[]
            blocks=[]
            for (w,h) in sizes:
                for name,fr,core,ms,follows in CH:
                    i=int(t//ms)%len(fr); c=col(t); rs=rows(w,h,fr[i],core[i],c,c if follows else PURPLE)
                    rs=[""]*((H-h)//2)+rs+[""]*(H-h-(H-h)//2)
                    blocks.append(rs+[f"\x1b[38;2;142;162;196m{(name+' '+str(w)+'×'+str(h)):^{w}}{RESET}"])
            for r in range(H+1):
                sys.stdout.write("   "+"      ".join(b[r] for b in blocks)+"\x1b[K\n")
            sys.stdout.write(f"\x1b[{H+1}A"); sys.stdout.flush(); time.sleep(0.033)
    except KeyboardInterrupt: pass
    finally: sys.stdout.write(f"\x1b[{H+1}B\x1b[?25h{RESET}\n")
if __name__=="__main__": main()
