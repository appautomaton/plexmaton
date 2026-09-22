import os, tempfile
OUT = os.path.join(tempfile.gettempdir(), "plexmaton-conversation-chrome")
os.makedirs(OUT, exist_ok=True)
# The user's message in the transcript: candidates, in the real frame's context and palette.
import html
P = dict(ground="#1C2233", band="#242D45", band_sel="#2C3754", line="#3D4664", muted="#8EA2C4", text="#E6E9F0",
         yellow="#F5D072", blue="#82B4F0", purple="#8B5CF6", green="#8CDAA5")
Q1 = "what is the latest news related to the qwen's image 2.1 model?"
Q2 = "and compare it with flux 2 on licensing, please keep it short, I only care whether I can ship it in a commercial app"
A1 = "Finding the latest on Qwen Image 2.1, checking fresh announcements."
A2 = "Just surfaced: released September 20th. Pulling the detailed specs and license terms."
A3 = "Qwen Image 2.1 ships under Apache 2.0 with a 20B parameter editing model; the base weights are open."

def wrap(s, w):
    out, line = [], ""
    for word in s.split(" "):
        if line and len(line) + 1 + len(word) > w: out.append(line); line = word
        else: line = word if not line else line + " " + word
    if line: out.append(line)
    return out

def esc(s): return html.escape(s)
def pad(s, n): return s + " " * max(0, n - len(s))

def user_rows(text, inner, style):
    """inner = columns between the frame's │ borders."""
    rows = []
    if style == "current":   # ▌ in accent yellow, text follows with no space
        for i, l in enumerate(wrap(text, inner - 1)):
            rows.append(f'<span class="yellow">▌</span><span class="text">{esc(pad(l, inner-1))}</span>')
    elif style == "band":    # lighter ground spanning the width, one column of air each side, no glyph
        for l in wrap(text, inner - 2):
            rows.append(f'<span class="band"> <span class="text">{esc(pad(l, inner-2))}</span> </span>')
    elif style == "prefix":  # quiet › then the text; continuation indents under the text
        ls = wrap(text, inner - 2)
        rows.append(f'<span class="muted">› </span><span class="text">{esc(pad(ls[0], inner-2))}</span>')
        rows += [f'  <span class="text">{esc(pad(l, inner-2))}</span>' for l in ls[1:]]
    elif style == "hairline": # the bar stays, thin and in the line colour, with a space after it
        for l in wrap(text, inner - 2):
            rows.append(f'<span class="line">▏</span> <span class="text">{esc(pad(l, inner-2))}</span>')
    elif style == "band-prefix":
        ls = wrap(text, inner - 4)
        rows.append(f'<span class="band"> <span class="blue">›</span> <span class="text">{esc(pad(ls[0], inner-4))}</span> </span>')
        rows += [f'<span class="band">   <span class="text">{esc(pad(l, inner-4))}</span> </span>' for l in ls[1:]]
    return rows

def frame(width, style):
    inner = width - 2
    body = []
    body += user_rows(Q1, inner, style)
    body.append("")
    body += [f'<span class="text">{esc(pad(l, inner))}</span>' for l in wrap(A1, inner)]
    body.append(f'<span class="purple">[+]</span> <span class="text">web_search</span>{" "*(inner-14)}')
    body += [f'<span class="text">{esc(pad(l, inner))}</span>' for l in wrap(A2, inner)]
    body.append("")
    body += user_rows(Q2, inner, style)
    body.append("")
    body += [f'<span class="text">{esc(pad(l, inner))}</span>' for l in wrap(A3, inner)]
    body.append("")
    act = f'<span class="muted">· </span><span class="text">Thinking…</span> <span class="muted">· 4s · high effort</span>'
    body.append(act + " " * (inner - 2 - len("Thinking… · 4s · high effort") - 7) + '<span class="muted">( !1 ) </span>')
    rows = [f'<div class="row">│{r if r else " "*inner}│</div>' for r in body]
    title = "── Message Agent A · primary " + "─" * (width - 29)
    hint = " Type a message · ⇥ to focus" + " " * (width - 28)
    rows += [f'<div class="row line">{title}</div>', f'<div class="row muted">{esc(hint)}</div>', f'<div class="row line">{"─"*width}</div>']
    return f'<div class="term" style="width:{width}ch">' + "".join(rows) + "</div>"

STYLES = [
 ("0  today: thick bar in the accent yellow", "current", ""),
 ("A  band: the user's turn sits on a lifted ground, nothing else marks it  ← recommended", "band",
  "Who is speaking is said by the surface, not by a glyph or a colour. Wrapped lines need no repeated gutter, nothing has to line up, and the accent yellow goes back to meaning attention."),
 ("B  quiet prefix ›", "prefix", "A single muted chevron, continuation indented under the text. Cheapest change; keeps the plain ground."),
 ("C  hairline: the bar stays, thin and in the line colour", "hairline", "Keeps today's structure with the loudness removed."),
 ("D  band with a blue › inside", "band-prefix", "A and B together, for comparison; probably one mark too many."),
]
out = [f'''<!doctype html><meta charset="utf-8"><title>user message · candidates</title><style>
body{{background:#0f131c;color:{P["text"]};font:14px/1.35 "Sarasa Term SC Nerd","Sarasa Term SC",Menlo,monospace;margin:24px}}
.term{{background:{P["ground"]};padding:10px 12px;border-radius:8px;display:inline-block;margin:6px 12px 14px 0;white-space:pre;vertical-align:top}}
.row{{white-space:pre;color:{P["line"]}}} .muted{{color:{P["muted"]}}} .line{{color:{P["line"]}}} .text{{color:{P["text"]}}} .purple{{color:{P["purple"]}}} .yellow{{color:{P["yellow"]}}} .blue{{color:{P["blue"]}}}
.band{{background:{P["band"]}}} h2{{font-weight:500;margin:26px 0 4px}} .note{{color:{P["muted"]};max-width:980px;margin:0 0 8px}}
</style><h1 style="font-weight:500">The user's message in the transcript</h1>
<p class="note">Same conversation in every block: two questions, two narration lines, one provider-run search, one answer, the activity line. The second question wraps at every width, which is where a gutter treatment shows its seams.</p>''']
for name, style, note in STYLES:
    out.append(f'<h2>{esc(name)}</h2>' + (f'<p class="note">{esc(note)}</p>' if note else ""))
    out.append(frame(120, style) + frame(60, style))
open(os.path.join(OUT, 'user-row.html'), 'w').write("\n".join(out)); print("ok")
print(os.path.join(OUT, 'user-row.html'))
