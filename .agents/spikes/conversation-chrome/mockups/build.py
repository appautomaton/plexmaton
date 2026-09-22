import os, tempfile
OUT = os.path.join(tempfile.gettempdir(), "plexmaton-conversation-chrome")
os.makedirs(OUT, exist_ok=True)
# Animated mockup of the conversation's activity line, in Plexmaton's palette.
# Rows are candidates; the rest of the frame is copied from crates/plexmaton-tui/frames/current-work-*.txt.
import json, html
P = dict(ground="#1C2233", line="#3D4664", muted="#8EA2C4", text="#E6E9F0", red="#FF7878", orange="#FFC466",
         yellow="#F5D072", green="#8CDAA5", cyan="#78D2CD", blue="#82B4F0", purple="#8B5CF6", magenta="#D946EF")
# name, glyph frames, tick ms, bounce?, spinner colour role, verb shimmer?
SPINNERS = [
  ("A  Claude Code: star bounce (12 frames, 120 ms), glyph and verb share one colour, verb shimmers", ["·","✢","✳","✶","✻","✽"], 120, True, "blue", True),
  ("B  logo: square with breathing core", ["▣"],                        67,  False, "core",   True),
  ("C  logo: square ⇄ circle",           ["▢","▣","■","◉","●","◉","■","▣"], 160, False, "blue", False),
  ("D  effort markers (already in the rail)", ["▲","■","⬢","●"],        400, False, "cyan",   False),
  ("E  braille dots",                    list("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),        80,  False, "blue",   False),
  ("F  quarter circle",                  ["◐","◓","◑","◒"],            120, False, "cyan",   False),
  ("G  Grok CLI: braille, phase timer left, turn timer right", ["⠋","⠙","⠹","⠸","⠼","⠴","⠦","⠧"], 133, False, "muted", False),
]
STATES = [
  ("Thinking…",            "32s · high effort",        "text"),
  ("Responding…",          "1m 04s · high effort",     "text"),
  ("Running read_file…",   "4s",                       "text"),
  ("Searching the web…",   "12s · high effort",        "purple"),
  ("Approval required",    "",                         "orange"),   # static, no spinner, action colour
  ("Compacting…",          "8s",                       "text"),
  ("Thinking…",            "32s · high effort · quiet for 31s", "text"),   # nothing has arrived for a while: say so, do not turn red
]
WIDTHS = [120, 88, 60]
def frame_rows(width, activity_html):
    rule = "─" * width
    title = "── Message Agent A · primary " + "─" * (width - 29)
    hint = " Type a message · ⇥ to focus" + " " * (width - 28)
    return (f'<div class="row muted">│<span class="text">remains interactive.</span>{" " * (width-22)}│</div>'
            f'<div class="row">│{activity_html}</div>'
            f'<div class="row line">{html.escape(title)}</div>'
            f'<div class="row muted">{html.escape(hint)}</div>'
            f'<div class="row line">{html.escape(rule)}</div>')
def activity(width, si, st):
    verb, meta, vrole = st
    inner = width - 2
    pill = "( !1 ) "
    if SPINNERS[si][0].startswith("G"):
        phase = {"Thinking…":"12s","Responding…":"4s","Running read_file…":"0.2s","Searching the web…":"3s","Approval required":"","Compacting…":"8s"}[verb]
        turn = "1m20s"
        spin = f'<span class="spin" data-s="{si}"></span> ' if verb != "Approval required" else '<span class="orange spin-pulse">◆</span> '
        left = f'{spin}<span class="verb {vrole}">{html.escape(verb)}</span>' + (f' <span class="muted">{phase}</span>' if phase else "")
        left_len = 2 + len(verb) + (1 + len(phase) if phase else 0)
        right = f'<span class="muted">{turn} ⇣12k </span><span class="muted">{pill}</span>'
        gap = max(1, inner - left_len - len(turn) - 6 - len(pill))
        return f'{left}{" " * gap}{right}│'
    spin = f'<span class="spin" data-s="{si}"></span> ' if verb != "Approval required" else '<span class="muted">· </span>'
    metah = f' <span class="muted">· {html.escape(meta)}</span>' if meta else ""
    left_len = 2 + len(verb) + (3 + len(meta) if meta else 0)
    gap = max(1, inner - left_len - len(pill))
    vclass = SPINNERS[si][4] if (SPINNERS[si][0].startswith("A") and vrole == "text") else vrole
    verb_h = f'<span class="verb {vclass}" data-shimmer="{int(SPINNERS[si][5])}">{html.escape(verb)}</span>'
    return f'{spin}{verb_h}{metah}{" " * gap}<span class="muted">{pill}</span>│'
out = ['<!doctype html><meta charset="utf-8"><title>Plexmaton · activity line candidates</title><style>',
  f'body{{background:#0f131c;color:{P["text"]};font:14px/1.35 "Sarasa Term SC Nerd","Sarasa Term SC",Menlo,monospace;margin:24px}}',
  f'.term{{background:{P["ground"]};padding:10px 12px;border-radius:8px;display:inline-block;margin:6px 0 18px;white-space:pre}}',
  f'.row{{white-space:pre}} .muted{{color:{P["muted"]}}} .line{{color:{P["line"]}}} .text{{color:{P["text"]}}} .purple{{color:{P["purple"]}}} .orange{{color:{P["orange"]}}} .blue{{color:{P["blue"]}}} .cyan{{color:{P["cyan"]}}}',
  'h2{font-weight:500;margin:22px 0 4px} h3{font-weight:400;color:#8EA2C4;margin:14px 0 2px;font-size:13px} .note{color:#8EA2C4;max-width:900px}',
  '</style><h1 style="font-weight:500">Activity line · candidates</h1>',
  '<p class="note">Same frame as the real <code>current-work-*</code> fixtures; only the activity row changes. Spinner ticks on the effort rail\'s 67 ms clock. Elapsed is wall time since the step began; effort is the configured level. Token counts are not shown: Responses reports usage only at the end of a turn, and the row claims only what was reported.</p>']
for si, (name, glyphs, tick, bounce, role, shimmer) in enumerate(SPINNERS):
    out.append(f'<h2>{html.escape(name)}</h2>')
    for w in WIDTHS:
        out.append(f'<h3>{w} columns</h3><div class="term" style="width:{w}ch">')
        for st in STATES:
            out.append(frame_rows(w, activity(w, si, st)))
        out.append('</div>')
out.append('<script>const S=' + json.dumps([dict(g=g,t=t,b=b,r=r) for _,g,t,b,r,_ in SPINNERS]) + ';const P=' + json.dumps(P) + ';')
out.append('''
const start=performance.now();
function core(t){ // breathing colour for the logo core: blue -> cyan -> purple and back
  const ph=(t/1600)%1, k=ph<0.5?ph*2:2-ph*2; const a=hex(P.blue), b=hex(P.purple);
  return 'rgb('+a.map((v,i)=>Math.round(v+(b[i]-v)*k)).join(',')+')'; }
function hex(h){return [1,3,5].map(i=>parseInt(h.substr(i,2),16));}
function tick(){ const t=performance.now()-start;
  document.querySelectorAll('.spin').forEach(el=>{ const s=S[el.dataset.s]; let n=Math.floor(t/s.t);
    const L=s.g.length; let i; if(s.b&&L>1){ const cyc=2*L-2; i=n%cyc; if(i>=L) i=cyc-i; } else i=n%L;
    el.textContent=s.g[i]; el.style.color = s.r==='core'?core(t):P[s.r]; });
  document.querySelectorAll('.verb[data-shimmer="1"]').forEach(el=>{ // Claude Code style shimmer: a highlight sweeps the verb
    const txt=el.textContent; if(!el.dataset.raw) el.dataset.raw=txt; const raw=el.dataset.raw; const pos=((t/60)%(raw.length+8))-4;
    el.innerHTML=[...raw].map((c,i)=>{const d=Math.abs(i-pos); const base=getComputedStyle(el).color; const col=d<1.5?P.text:(d<3?'#C9D6F2':base); return '<span style="color:'+col+'">'+c.replace('<','&lt;')+'</span>';}).join(''); });
  requestAnimationFrame(tick);} tick();</script>''')
open(os.path.join(OUT, 'index.html'), 'w').write('\n'.join(out))
print('ok')
print(os.path.join(OUT, 'index.html'))
