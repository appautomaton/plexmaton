import os, tempfile
OUT = os.path.join(tempfile.gettempdir(), "plexmaton-conversation-chrome")
os.makedirs(OUT, exist_ok=True)
# Animated mark: frame thickness breathes, frame colour drifts between palette slots, core is solid or a ring.
import json
P = dict(ground="#1C2233", line="#3D4664", muted="#8EA2C4", text="#E6E9F0", blue="#82B4F0", cyan="#78D2CD", purple="#8B5CF6", magenta="#D946EF")
# cell frames by thickness: light rounded -> heavy -> half-block
def frame_rows(w, h, level):
    if level == 0: t, b, m, tl, tr, bl, br = "─","─","│","╭","╮","╰","╯"
    elif level == 1: t, b, m, tl, tr, bl, br = "━","━","┃","┏","┓","┗","┛"
    else: t, b, m, tl, tr, bl, br = "▀","▄","▌","▛","▜","▙","▟"
    right = "▐" if level == 2 else m
    rows = [tl + t*(w-2) + tr] + [m + " "*(w-2) + right for _ in range(h-2)] + [bl + b*(w-2) + br]
    return rows
CHOREO = [
 ("K1  breathe together: frame thickens as the core fills, colour drifts blue → purple → magenta and back",
  dict(frame=[0,0,1,1,2,2,1,1], core=["○","○","◉","●","●","●","◉","○"], ms=220, core_follows=False)),
 ("K2  still frame, the core does the moving: ring grows, fills, opens again (C2′ inside the logo)",
  dict(frame=[0]*8, core=["◦","○","◯","●","◉","●","◯","○"], ms=180, core_follows=False)),
 ("K3  counterpoint: frame thick when the core is a dot, thin when the core is a wide ring",
  dict(frame=[2,2,1,0,0,0,1,2], core=["·","◦","○","◯","◯","◯","○","◦"], ms=200, core_follows=True)),
 ("K4  the core morphs: ring → rounded square → outline square → back, then solid circle → small solid square → solid circle",
  dict(frame=[0]*8, core=["○","▢","◻","▢","○","●","▪","●"], ms=200, core_follows=False)),
]
SIZES = [(7,3),(9,5),(11,5)]   # odd on both axes: the core has one exact centre cell; even sizes are not shown
def cell_block(ci, w, h):
    return f'<pre class="cells" data-c="{ci}" data-w="{w}" data-h="{h}"></pre>'
out=[f'''<!doctype html><meta charset="utf-8"><title>mark in motion</title><style>
body{{background:#0f131c;color:{P["text"]};font:14px/1.4 "Sarasa Term SC Nerd",Menlo,monospace;margin:24px}}
.card{{background:{P["ground"]};border-radius:10px;padding:18px 22px;display:inline-block;margin:6px 10px 14px 0;vertical-align:top;text-align:center;min-width:120px}}
h2{{font-weight:500;margin:26px 0 6px;max-width:980px}} .note{{color:{P["muted"]};max-width:980px}} .muted{{color:{P["muted"]};font-size:12px;margin-top:8px}}
pre{{margin:0;font-size:18px;line-height:1.1}} pre .core{{}} 
</style><h1 style="font-weight:500">The mark in motion</h1>
<p class="note">Left card: the SVG ideal, stroke width and colour animate continuously; the core alternates solid and ring. Right cards: the same choreography in terminal cells, where thickness has three steps (light rounded line, heavy line, half-block) and the colour is one palette slot drifting toward the next. Everything runs on one clock; nothing here is a new colour. Only odd cell sizes are shown: a core can sit in the exact centre only when width and height are odd, so 6×3 and 8×5 are gone. Terminal cells are about twice as tall as wide, so 7×3 and 11×5 read close to square, 9×5 slightly tall.</p>''']
for ci,(name,c) in enumerate(CHOREO):
    out.append(f'<h2>{name}</h2><div class="card"><svg class="svg" data-c="{ci}" width="160" height="160" viewBox="0 0 160 160"><rect class="fr" x="8" y="8" width="144" height="144" rx="36" fill="none" stroke="{P["blue"]}" stroke-width="10"/><circle class="co" cx="80" cy="80" r="34" fill="{P["purple"]}" stroke="{P["purple"]}" stroke-width="10"/><rect class="sq" x="46" y="46" width="68" height="68" rx="34" fill="none" stroke="none" stroke-width="10"/></svg><div class="muted">SVG</div></div>')
    for w,h in SIZES:
        out.append(f'<div class="card">{cell_block(ci,w,h)}<div class="muted">{w}×{h} cells</div></div>')
out.append(f'''<h2>In the activity line, while the agent works</h2>
<p class="note">The activity line is one row, so the logo cannot stand there whole. Two honest options. One cell: the mark reduced to a single glyph, the frame ▢ and the core taking turns; ▣ and ◉ are the only glyphs that hold a frame and a core together. Three rows: the 7×3 logo beside the text, which costs the conversation two rows whenever the agent is working.</p>
<div class="card" style="text-align:left"><pre style="font-size:16px"><span class="one" data-seq="▫◻□▢◯○◦○◯▢□◻"></span> Thinking… <span style="color:{P["muted"]}">· 32s · high effort</span></pre><div class="muted">R1 · C2′, the frame becoming the core</div></div>
<div class="card" style="text-align:left"><pre style="font-size:16px"><span class="one" data-seq="▢▣▢◯◉◯"></span> Thinking… <span style="color:{P["muted"]}">· 32s · high effort</span></pre><div class="muted">R2 · frame with a core, then circle with a core</div></div>
<div class="card" style="text-align:left"><pre style="font-size:16px"><span class="one" data-seq="○▢◻▢○●▪●"></span> Thinking… <span style="color:{P["muted"]}">· 32s · high effort</span></pre><div class="muted">R3 · K4's core alone</div></div>
<div class="card" style="text-align:left"><pre class="cells" data-c="3" data-w="7" data-h="3" style="display:inline-block;vertical-align:middle;font-size:16px"></pre><pre style="display:inline-block;vertical-align:middle;font-size:16px;margin-left:10px"> Thinking… <span style="color:{P["muted"]}">· 32s · high effort</span></pre><div class="muted">R4 · the 7×3 logo beside the text, three rows tall</div></div>''')
out.append('<script>const P='+json.dumps(P)+';const C='+json.dumps([c for _,c in CHOREO])+';')
out.append(r'''
const hex=h=>[1,3,5].map(i=>parseInt(h.substr(i,2),16));
const DRIFT=[P.blue,P.purple,P.magenta,P.purple,P.blue,P.cyan]; // one slot at a time, drifting to the next
function drift(t,period){const n=DRIFT.length,x=(t/period)%n,i=Math.floor(x),k=x-i,a=hex(DRIFT[i]),b=hex(DRIFT[(i+1)%n]);return 'rgb('+a.map((v,j)=>Math.round(v+(b[j]-v)*k)).join(',')+')';}
function rows(w,h,level){const L=[["─","─","│","╭","╮","╰","╯","│"],["━","━","┃","┏","┓","┗","┛","┃"],["▀","▄","▌","▛","▜","▙","▟","▐"]][level];const [t,b,m,tl,tr,bl,br,r]=L;const out=[tl+t.repeat(w-2)+tr];for(let i=0;i<h-2;i++)out.push(m+" ".repeat(w-2)+r);out.push(bl+b.repeat(w-2)+br);return out;}
const start=performance.now();
function tick(){const t=performance.now()-start;
  document.querySelectorAll('.cells').forEach(el=>{const c=C[el.dataset.c],w=+el.dataset.w,h=+el.dataset.h;const i=Math.floor(t/c.ms)%c.frame.length;const col=drift(t,2500);
    const rs=rows(w,h,c.frame[i]);const mid=Math.floor((h-1)/2),cx=Math.floor((w-1)/2);
    const coreCol=c.core_follows?col:P.purple;
    const html=rs.map((r,ri)=>{if(ri!==mid)return r;return r.slice(0,cx)+'<span style="color:'+coreCol+'">'+c.core[i]+'</span>'+r.slice(cx+1);}).join('\n');
    el.innerHTML=html;el.style.color=col;});
  document.querySelectorAll('.one').forEach(el=>{const q=[...el.dataset.seq];el.textContent=q[Math.floor(t/200)%q.length];el.style.color=drift(t,2500);});
  document.querySelectorAll('.svg').forEach(el=>{const c=C[el.dataset.c];const ph=(t/(c.ms*c.frame.length))%1;const breathe=0.5-0.5*Math.cos(ph*2*Math.PI);
    const fr=el.querySelector('.fr'),co=el.querySelector('.co');const col=drift(t,2500);
    const thick=(el.dataset.c==='1')?10:6+14*breathe; fr.setAttribute('stroke-width',thick);fr.setAttribute('stroke',col);
    const solid=(el.dataset.c==='2')?breathe<0.5:breathe>0.5; co.setAttribute('fill',solid?(c.core_follows?col:P.purple):'none');co.setAttribute('stroke',c.core_follows?col:P.purple);
    co.setAttribute('r',(el.dataset.c==='2')?20+22*(1-breathe):(el.dataset.c==='1'?18+20*breathe:34));
    const sq=el.querySelector('.sq'); if(el.dataset.c==='3'){ co.setAttribute('fill','none'); co.setAttribute('stroke','none');
      const half=ph<0.5; const m=half?(0.5-0.5*Math.cos(ph*4*Math.PI)):(0.5-0.5*Math.cos((ph-0.5)*4*Math.PI)); // 0..1..0 twice per cycle
      sq.setAttribute('rx',34-22*m); sq.setAttribute('stroke',P.purple); sq.setAttribute('fill',half?'none':P.purple); } else { sq.setAttribute('stroke','none'); sq.setAttribute('fill','none'); }});
  requestAnimationFrame(tick);}tick();</script>''')
open(os.path.join(OUT, 'logo-motion.html'), 'w').write("\n".join(out)); print("ok")
print(os.path.join(OUT, 'logo-motion.html'))
