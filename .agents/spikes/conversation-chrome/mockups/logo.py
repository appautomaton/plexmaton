import os, tempfile
OUT = os.path.join(tempfile.gettempdir(), "plexmaton-conversation-chrome")
os.makedirs(OUT, exist_ok=True)
# The mark as stage 10 describes it: a rounded-square frame with a circular gradient centre, in the palette.
# No reference image has arrived, so these are proportions to choose between, not a redraw of yours.
P = dict(ground="#1C2233", line="#3D4664", muted="#8EA2C4", text="#E6E9F0", blue="#82B4F0", purple="#8B5CF6", cyan="#78D2CD", magenta="#D946EF")
def mark(size, frame_pct, radius_pct, core_pct, style):
    s=size; f=s*frame_pct; r=s*radius_pct; c=s*core_pct/2; cx=cy=s/2
    grad=f'<radialGradient id="g{style}{size}" cx="45%" cy="40%" r="65%"><stop offset="0" stop-color="{P["magenta"]}"/><stop offset=".45" stop-color="{P["purple"]}"/><stop offset="1" stop-color="{P["blue"]}"/></radialGradient>'
    fgrad=f'<linearGradient id="f{style}{size}" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="{P["blue"]}"/><stop offset="1" stop-color="{P["purple"]}"/></linearGradient>'
    if style=="outline":   # thin frame, gradient core
        body=f'<rect x="{f/2}" y="{f/2}" width="{s-f}" height="{s-f}" rx="{r}" fill="none" stroke="{P["blue"]}" stroke-width="{f}"/><circle cx="{cx}" cy="{cy}" r="{c}" fill="url(#g{style}{size})"/>'
    elif style=="filled":  # solid rounded square, the core is a window (the ◙ reading)
        body=f'<rect x="0" y="0" width="{s}" height="{s}" rx="{r}" fill="{P["blue"]}"/><circle cx="{cx}" cy="{cy}" r="{c}" fill="url(#g{style}{size})"/>'
    elif style=="gradient-frame":  # frame carries the gradient, core solid text colour
        body=f'<rect x="{f/2}" y="{f/2}" width="{s-f}" height="{s-f}" rx="{r}" fill="none" stroke="url(#f{style}{size})" stroke-width="{f}"/><circle cx="{cx}" cy="{cy}" r="{c}" fill="{P["text"]}"/>'
    else:  # ring core: frame + a ring, echoing the spinner's ◯ inside ▢
        body=f'<rect x="{f/2}" y="{f/2}" width="{s-f}" height="{s-f}" rx="{r}" fill="none" stroke="{P["blue"]}" stroke-width="{f}"/><circle cx="{cx}" cy="{cy}" r="{c}" fill="none" stroke="url(#g{style}{size})" stroke-width="{f}"/>'
    return f'<svg width="{s}" height="{s}" viewBox="0 0 {s} {s}"><defs>{grad}{fgrad}</defs>{body}</svg>'
V=[("L1  thin frame, gradient core","outline",0.07,0.24,0.46),
   ("L2  heavier frame, smaller core","outline",0.11,0.28,0.38),
   ("L3  solid square, core as a window","filled",0.0,0.26,0.44),
   ("L4  gradient on the frame, plain core","gradient-frame",0.08,0.24,0.40),
   ("L5  frame with a ring core, the spinner's ▢ and ◯ together","ring",0.07,0.24,0.46)]
cells = [("4×3", ["╭──╮","│<b>●</b> │","╰──╯"]), ("6×3", ["╭────╮","│ <b>◉</b>  │","╰────╯"]), ("8×5", ["╭──────╮","│      │","│  <b>◉</b>   │","│      │","╰──────╯"])]
out=[f'''<!doctype html><meta charset="utf-8"><title>mark</title><style>
body{{background:#0f131c;color:{P["text"]};font:14px/1.4 "Sarasa Term SC Nerd",Menlo,monospace;margin:24px}}
.card{{background:{P["ground"]};border-radius:10px;padding:18px 22px;display:inline-block;margin:6px 10px 14px 0;vertical-align:top;text-align:center}}
.card svg{{display:block;margin:0 auto 10px}} .row svg{{vertical-align:middle;margin-right:14px}}
h2{{font-weight:500;margin:26px 0 6px}} .note{{color:{P["muted"]};max-width:960px}} .muted{{color:{P["muted"]}}}
pre{{margin:0;font-size:16px;line-height:1.15}} pre b{{color:{P["purple"]};font-weight:normal}} pre{{color:{P["blue"]}}}
</style><h1 style="font-weight:500">The mark, from the words in stage 10</h1>
<p class="note">"A rounded-square frame and circular gradient centre." Your image did not arrive, so these are five proportions of that sentence in the palette, each at 160, 48 and 20 px, then what the same mark can be inside terminal cells. Pick one to refine, or give me the path of your image on disk and I will read it from there.</p>''']
for name,style,f,r,c in V:
    out.append(f'<h2>{name}</h2><div class="card">{mark(160,f,r,c,style)}</div><div class="card" style="vertical-align:bottom">{mark(48,f,r,c,style)}</div><div class="card" style="vertical-align:bottom">{mark(20,f,r,c,style)}</div>')
out.append('<h2>In terminal cells</h2><p class="note">A header or drawer can afford a few cells: box-drawing corners make the rounded frame, one glyph is the core. The one-cell spinner is the same mark reduced: ▢ is the frame, ◯ ○ ◦ the core breathing, which is why C2′ reads as the logo moving.</p>')
for label,rows in cells:
    out.append(f'<div class="card"><pre>{"<br>".join(rows)}</pre><div class="muted" style="margin-top:8px">{label}</div></div>')
out.append(f'<div class="card"><pre style="font-size:40px;line-height:1">▢</pre><div class="muted">1×1, the spinner\'s frame</div></div>')
open(os.path.join(OUT, 'logo.html'), 'w').write("\n".join(out)); print("ok")
print(os.path.join(OUT, 'logo.html'))
