import os, tempfile
OUT = os.path.join(tempfile.gettempdir(), "plexmaton-conversation-chrome")
os.makedirs(OUT, exist_ok=True)
# The mark drawn with Nerd Font (Material Design Icons) shapes: one icon family, one cell each, designed together.
import json
P = dict(ground="#1C2233", line="#3D4664", muted="#8EA2C4", text="#E6E9F0", blue="#82B4F0", cyan="#78D2CD", purple="#8B5CF6", magenta="#D946EF")
G = dict(circle_small="\U000F09DF", circle_medium="\U000F09DE", circle="\U000F0765", circle_outline="\U000F0766",
         square="\U000F0763", square_outline="\U000F0764", square_rounded="\U000F14FB", square_rounded_outline="\U000F14FC",
         radiobox_blank="\U000F043D", radiobox_marked="\U000F043E", record="\U000F044A", record_circle="\U000F0FEC", circle_double="\U000F0E95",
         checkbox_blank_circle="\U000F012F", checkbox_blank_circle_outline="\U000F0130", checkbox_blank="\U000F012E", checkbox_blank_outline="\U000F0131")
SEQ = [
 ("N1  core morph, solid: small dot → circle → rounded square → square → back", [G["circle_small"],G["circle_medium"],G["circle"],G["square_rounded"],G["square"],G["square_rounded"],G["circle"],G["circle_medium"]]),
 ("N2  core morph, outline: circle → rounded square → square → back", [G["circle_outline"],G["square_rounded_outline"],G["square_outline"],G["square_rounded_outline"]]),
 ("N3  fill and empty: solid circle ⇄ ring, solid rounded square ⇄ rounded frame", [G["circle"],G["circle_outline"],G["square_rounded_outline"],G["square_rounded"],G["square_rounded_outline"],G["circle_outline"]]),
 ("N4  frame and core in one cell: ring with dot, record, double ring", [G["radiobox_marked"],G["record_circle"],G["circle_double"],G["record_circle"]]),
]
# the multi-cell logo; the core is a Nerd Font glyph in the exact centre cell
def rows(w,h,level):
    L=[("─","─","│","╭","╮","╰","╯","│"),("━","━","┃","┏","┓","┗","┛","┃"),("▀","▄","▌","▛","▜","▙","▟","▐")][level]
    t,b,m,tl,tr,bl,br,r=L; return [tl+t*(w-2)+tr]+[m+" "*(w-2)+r for _ in range(h-2)]+[bl+b*(w-2)+br]
SIZES=[(7,3),(11,5),(13,5),(15,7)]
ADV,LINE=0.6,1.32   # JetBrains Mono, measured from the CDN file; kitty's default line height adds nothing
out=[f'''<!doctype html><meta charset="utf-8"><title>mark · Nerd Font shapes</title><style>
@font-face{{font-family:"JBM";src:url("https://cdn.jsdelivr.net/fontsource/fonts/jetbrains-mono@latest/latin-400-normal.woff2") format("woff2")}}
@font-face{{font-family:"NFS";src:url("https://cdn.jsdelivr.net/gh/ryanoasis/nerd-fonts@master/patched-fonts/NerdFontsSymbolsOnly/SymbolsNerdFontMono-Regular.ttf")}}
body{{background:#0f131c;color:{P["text"]};font:14px/1.32 "JBM",Menlo,monospace;margin:24px}}
.nf{{font-family:"NFS";display:inline-block;width:1ch;text-align:center;font-size:.62em;vertical-align:baseline;position:relative;top:-.08em}}
pre .nf, .row .nf{{font-size:.62em;top:-.08em}}
.card{{background:{P["ground"]};border-radius:10px;padding:16px 20px;display:inline-block;margin:6px 10px 14px 0;vertical-align:top;text-align:center}}
h2{{font-weight:500;margin:26px 0 6px;max-width:980px}} .note{{color:{P["muted"]};max-width:980px}} .muted{{color:{P["muted"]};font-size:12px;margin-top:8px}}
.strip span{{display:inline-block;width:1ch;text-align:center;font-size:44px;line-height:1.32;outline:1px dashed {P["line"]};margin:0 8px 0 0;color:{P["blue"]};font-family:"JBM"}} .strip span i{{font-family:"NFS";font-style:normal;font-size:.62em;position:relative;top:-.08em}}
.big{{font-size:64px;line-height:1.32;display:inline-block;width:1ch;outline:1px dashed {P["line"]};margin-right:22px;vertical-align:middle;text-align:center;font-family:"JBM"}} .big i{{font-family:"NFS";font-style:normal;font-size:.62em;position:relative;top:-.08em}}
pre{{margin:0;font-size:16px;line-height:1.32;color:{P["blue"]};font-family:"JBM"}} pre b{{color:{P["purple"]};font-weight:normal;font-family:"NFS";font-size:.62em;position:relative;top:-.08em;display:inline-block;width:1ch;text-align:center}}
</style><h1 style="font-weight:500">The mark with Nerd Font shapes</h1>
<p class="note">Text is JetBrains Mono and the shapes are Symbols Nerd Font Mono, both loaded from a CDN, nothing from your machine. The product already draws its header, tally and copy glyphs from this Material Design Icons set, so the mark adds no new dependency. Every shape is one icon family fitted to one cell, the way kitty fits a symbol font, so a morph does not jump between fonts. Each dashed box is one JetBrains Mono cell: 0.6 em wide, 1.32 em tall.</p>''']
for name,seq in SEQ:
    out.append(f'<h2>{name}</h2><div class="card" style="text-align:left"><span class="big spin" data-seq="{json.dumps(seq).replace(chr(34),"&quot;")}"></span><span class="strip">'+"".join(f'<span><i>{g}</i></span>' for g in seq)+'</span></div>')
out.append(f'<h2>The logo in cells, core exactly centred, sizes that come out square in this font</h2><p class="note">A JetBrains Mono cell is {ADV} em wide and {LINE} em tall, so a block is square when columns ≈ {LINE/ADV:.2f} × rows, and only odd counts keep the core centred. 11×5 is exactly square here; 7×3 and 15×7 are within 6 %. Whatever font you settle on, the app should ask the terminal for its cell size in pixels at startup and pick the odd width nearest square for the rows it has: a fixed 7×3 is only square for one font.</p>')
for w,h in SIZES:
    ratio=(w*ADV)/(h*LINE)
    out.append(f'<div class="card"><pre class="cells" data-w="{w}" data-h="{h}"></pre><div class="muted">{w}×{h} · width/height {ratio:.2f}</div></div>')
# where it could live first: the empty conversation, before the first message
def empty_state(width, w, h):
    inner=width-2; pad=(inner-w)//2
    body=[""]*3
    body+=[f'<span class="logo-row" data-w="{w}" data-h="{h}" data-i="{i}">{" "*pad}</span>' for i in range(h)]
    body+=["", f'<span class="text">{"Plexmaton".center(inner)}</span>', f'<span class="muted">{"a harness for agents that work while you watch".center(inner)}</span>', "", ""]
    rows=[f'<div class="row line">│{r if r else " "*inner}│</div>' for r in body]
    title="── Message Agent A · primary "+"─"*(width-29); hint=" Type a message · ⇥ to focus"+" "*(width-28)
    rows+=[f'<div class="row line">{title}</div>', f'<div class="row muted">{hint}</div>', f'<div class="row line">{"─"*width}</div>']
    return f'<div class="term" style="width:{width}ch">'+"".join(rows)+'</div>'
out.append('<h2>Where it could live first: the empty conversation</h2><p class="note">Before the first message the transcript is blank space. The mark sits centred there with the name under it, moving on the visible-only clock, and leaves when the first message arrives. Nothing else in the frame changes.</p>')
out.append('<style>.term{background:'+P["ground"]+';padding:10px 12px;border-radius:8px;display:inline-block;margin:6px 12px 14px 0;white-space:pre;vertical-align:top;font-size:14px} .row{white-space:pre;line-height:1.32} .line{color:'+P["line"]+'} .text{color:'+P["text"]+'} .muted{color:'+P["muted"]+'} .logo-row{color:'+P["blue"]+'} .logo-row b{color:'+P["purple"]+';font-weight:normal}</style>')
out.append(empty_state(120,11,5)+empty_state(60,7,3))
out.append('<script>const P='+json.dumps(P)+';const G='+json.dumps(G)+';')
out.append(r'''
const hex=h=>[1,3,5].map(i=>parseInt(h.substr(i,2),16));const DRIFT=[P.blue,P.purple,P.magenta,P.purple,P.blue,P.cyan];
function drift(t,period){const n=DRIFT.length,x=(t/period)%n,i=Math.floor(x),k=x-i,a=hex(DRIFT[i]),b=hex(DRIFT[(i+1)%n]);return 'rgb('+a.map((v,j)=>Math.round(v+(b[j]-v)*k)).join(',')+')';}
function rows(w,h,level){const L=[["─","─","│","╭","╮","╰","╯","│"],["━","━","┃","┏","┓","┗","┛","┃"],["▀","▄","▌","▛","▜","▙","▟","▐"]][level];const [t,b,m,tl,tr,bl,br,r]=L;const o=[tl+t.repeat(w-2)+tr];for(let i=0;i<h-2;i++)o.push(m+" ".repeat(w-2)+r);o.push(bl+b.repeat(w-2)+br);return o;}
const CORE=[G.circle_small,G.circle_medium,G.circle,G.square_rounded,G.square,G.square_rounded,G.circle,G.circle_medium];
const start=performance.now();
function tick(){const t=performance.now()-start;const col=drift(t,2500);
  document.querySelectorAll('.spin').forEach(el=>{const q=JSON.parse(el.dataset.seq);el.innerHTML='<i>'+q[Math.floor(t/220)%q.length]+'</i>';el.style.color=col;});
  document.querySelectorAll('.cells').forEach(el=>{const w=+el.dataset.w,h=+el.dataset.h;const lv=[0,0,1,1,2,2,1,1][Math.floor(t/300)%8];const rs=rows(w,h,lv);const mid=(h-1)/2,cx=(w-1)/2;
    const core=CORE[Math.floor(t/220)%CORE.length];el.innerHTML=rs.map((r,i)=>i!==mid?r:r.slice(0,cx)+'<b>'+core+'</b>'+r.slice(cx+1)).join('\n');el.style.color=col;});
  document.querySelectorAll('.logo-row').forEach(el=>{const w=+el.dataset.w,h=+el.dataset.h,i=+el.dataset.i;const lv=[0,0,1,1,2,2,1,1][Math.floor(t/300)%8];const rs=rows(w,h,lv);const mid=(h-1)/2,cx=(w-1)/2;const core=CORE[Math.floor(t/220)%CORE.length];
    const pad=el.textContent.length? el.dataset.pad||(el.dataset.pad=el.textContent.match(/^ */)[0].length):0; let r=rs[i]; if(i===mid) r=r.slice(0,cx)+'<b>'+core+'</b>'+r.slice(cx+1); el.innerHTML=" ".repeat(+el.dataset.pad)+r; el.style.color=col;});
  requestAnimationFrame(tick);}tick();</script>''')
open(os.path.join(OUT, 'mark-nerd.html'), 'w').write("\n".join(out)); print('ok')
print(os.path.join(OUT, 'mark-nerd.html'))
