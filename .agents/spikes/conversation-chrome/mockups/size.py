import os, tempfile
OUT = os.path.join(tempfile.gettempdir(), "plexmaton-conversation-chrome")
os.makedirs(OUT, exist_ok=True)
# Square <-> circle with size steps. Every glyph is one terminal column; none has emoji presentation.
import json, html
P = dict(ground="#1C2233", line="#3D4664", muted="#8EA2C4", text="#E6E9F0", orange="#FFC466", cyan="#78D2CD", blue="#82B4F0", purple="#8B5CF6")
V = [
 ("C1  filled: circle grows, becomes the square, square shrinks",
  ["·","•","●","⬤","■","◼","▪","◼","■","⬤","●","•"], 110, "blue"),
 ("C2  outline: ring grows, rounds into the frame, frame sharpens",
  ["∘","◦","○","◯","▢","□","◻","□","▢","◯","○","◦"], 110, "blue"),
 ("C2'  same idea, glyphs from the Geometric Shapes block only, so one font draws every frame",
  ["▫","◻","□","▢","◯","○","◦","○","◯","▢","□","◻"], 110, "blue"),
 ("C2'' three sizes only",
  ["◦","○","◯","▢","□","▢","◯","○"], 130, "blue"),
 ("C3  logo breathing: the square grows and a round core opens in it",
  ["▪","◼","■","◘","◙","◘","■","◼"], 120, "core"),
 ("C4  logo assembling: the dot grows, the frame closes around it, fills, opens again",
  ["·","•","●","◙","■","◙","●","•"], 120, "core"),
]
STATES = [("Thinking…","32s · high effort","text"),("Running read_file…","4s","text"),("Searching the web…","12s · high effort","purple")]
def row(width, vi, st):
    verb, meta, vrole = st; inner = width-2; pill="( !1 ) "
    left_len = 2+len(verb)+3+len(meta); gap=max(1, inner-left_len-len(pill))
    return (f'<div class="row">│<span class="cell spin" data-v="{vi}"></span> <span class="{vrole} verb">{html.escape(verb)}</span>'
            f' <span class="muted">· {html.escape(meta)}</span>{" "*gap}<span class="muted">{pill}</span>│</div>')
def frame(width, vi):
    title="── Message Agent A · primary "+"─"*(width-29); hint=" Type a message · ⇥ to focus"+" "*(width-28)
    rows=[f'<div class="row">│<span class="text">remains interactive.</span>{" "*(width-22)}│</div>']
    rows+= [row(width,vi,s) for s in STATES]
    rows+= [f'<div class="row line">{title}</div>', f'<div class="row muted">{hint}</div>', f'<div class="row line">{"─"*width}</div>']
    return f'<div class="term" style="width:{width}ch">'+"".join(rows)+'</div>'
out=[f'''<!doctype html><meta charset="utf-8"><title>square ⇄ circle · size steps</title><style>
body{{background:#0f131c;color:{P["text"]};font:14px/1.35 "Sarasa Term SC Nerd","Sarasa Term SC",Menlo,monospace;margin:24px}}
.term{{background:{P["ground"]};padding:10px 12px;border-radius:8px;display:inline-block;margin:6px 12px 18px 0;white-space:pre;vertical-align:top}}
.row{{white-space:pre}} .muted{{color:{P["muted"]}}} .line{{color:{P["line"]}}} .text{{color:{P["text"]}}} .purple{{color:{P["purple"]}}}
.cell{{display:inline-block;width:1ch;text-align:center}}
h2{{font-weight:500;margin:26px 0 6px}} .note{{color:{P["muted"]};max-width:960px}}
.strip{{background:{P["ground"]};border-radius:8px;padding:14px 18px;display:inline-block;margin:4px 0 10px}}
.strip .g{{display:inline-block;width:1ch;text-align:center;font-size:56px;line-height:1.1;margin-right:10px;position:relative}}
.strip .g::before{{content:"";position:absolute;inset:0;border:1px dashed #3D4664;pointer-events:none}}
.big{{font-size:72px;line-height:1.1;display:inline-block;width:1ch;text-align:center;position:relative;vertical-align:middle;margin:0 18px 0 0}}
.big::before{{content:"";position:absolute;inset:0;border:1px dashed #3D4664}}
</style><h1 style="font-weight:500">square ⇄ circle, with size steps</h1>
<p class="note">Each dashed box is one terminal cell. The terminal centres a glyph by its font, so the strip shows where each glyph really sits; a frame that looks off-centre here will look off-centre there too. Every glyph is one column wide and none has an emoji presentation, so no frame can jump to two cells. The big cell on the left plays the cycle; the strip is the same cycle laid out frame by frame.<br><br>
Why the first preview jumped: the browser took these glyphs from three different fonts, and each font centres its shapes on a different axis. This page now asks for the font kitty uses, Sarasa Term SC Nerd, first. The terminal is still the only truth, so the same cycles are in <code>spin.py</code> for kitty.</p>''']
for vi,(name,g,t,role) in enumerate(V):
    out.append(f'<h2>{html.escape(name)}</h2><div class="strip"><span class="big spin" data-v="{vi}"></span>'+"".join(f'<span class="g">{x}</span>' for x in g)+f'</div><div class="note">{len(g)} frames · {t} ms each · {len(g)*t/1000:.1f} s per cycle</div><br>')
    out.append(frame(120,vi)+frame(60,vi))
out.append('<script>const V='+json.dumps([dict(g=g,t=t,r=r) for _,g,t,r in V])+';const P='+json.dumps(P)+''';
const start=performance.now(); const hex=h=>[1,3,5].map(i=>parseInt(h.substr(i,2),16));
function core(t){const ph=(t/1600)%1,k=ph<0.5?ph*2:2-ph*2,a=hex(P.blue),b=hex(P.purple);return 'rgb('+a.map((v,i)=>Math.round(v+(b[i]-v)*k)).join(',')+')';}
function tick(){const t=performance.now()-start;document.querySelectorAll('.spin').forEach(el=>{const v=V[el.dataset.v];el.textContent=v.g[Math.floor(t/v.t)%v.g.length];el.style.color=v.r==='core'?core(t):P[v.r];});requestAnimationFrame(tick);}tick();</script>''')
open(os.path.join(OUT, 'size.html'), 'w').write("\n".join(out)); print('ok')
print(os.path.join(OUT, 'size.html'))
