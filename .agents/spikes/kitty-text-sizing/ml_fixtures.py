"""Fixed ML transport fixtures, not an implementation of TeX parsing or general math layout.

RLHF: fixed-prompt, KL-regularized expected reward; not the PPO clipped surrogate.
Attention: Attention Is All You Need, equation (1). References are in README.md.
"""
from transport import Align


SOURCES = {
    "rlhf": r"J(\theta)=E_{y\sim\pi_\theta(\cdot|x)}\left[r_\phi(x,y)-\beta\log\frac{\pi_\theta(y|x)}{\pi_{\mathrm{ref}}(y|x)}\right]",
    "attention": r"A(Q,K,V)=\operatorname{softmax}\left(\frac{QK^T}{\sqrt{d_k}}\right)V",
    "matrix": r"C=AB,\qquad C_{ij}=\sum_{k=1}^{n}A_{ik}B_{kj}",
}


def body(canvas, x, y, text):
    canvas.add(x, y, text, scale=(2, 1, 2, Align.CENTER))


def script(canvas, x, y, text, fraction=(2, 3), alignment=Align.TOP):
    canvas.add(x, y, text, scale=(1, *fraction, alignment))


def delimiters(canvas, left, right, y, height, round_brackets=False):
    parts = ("⎛⎜⎝", "⎞⎟⎠") if round_brackets else ("⎡⎢⎣", "⎤⎥⎦")
    for row in range(height):
        index = 0 if row == 0 else 2 if row == height - 1 else 1
        canvas.add(left, y + row, parts[0][index])
        canvas.add(right, y + row, parts[1][index])


def policy(canvas, x, y, index):
    body(canvas, x, y, "π")
    script(canvas, x + 2, y + 1, index)
    index_width = (len(index) * 2 + 2) // 3
    body(canvas, x + 2 + index_width, y, "(y|x)")


def rlhf(canvas):
    canvas.add(2, 4, "1. RLHF / KL-regularized expected reward", color=96)
    canvas.add(2, 5, "Fixed prompt x. This is not the PPO clipped loss.", color=90)
    body(canvas, 2, 9, "J(θ) =")
    body(canvas, 12, 9, "E")
    script(canvas, 9, 11, "y∼", (1, 2))
    script(canvas, 10, 11, "π", (1, 2))
    script(canvas, 11, 11, "θ", (1, 3), Align.BOTTOM)
    script(canvas, 12, 11, "(·|x)", (1, 2))
    delimiters(canvas, 18, 55, 7, 6)
    body(canvas, 20, 9, "r")
    script(canvas, 22, 10, "φ")
    body(canvas, 23, 9, "(x,y)")
    body(canvas, 30, 9, "− β log")
    policy(canvas, 40, 7, "θ")
    body(canvas, 39, 9, "──────────────")
    policy(canvas, 40, 11, "ref")


def attention(canvas):
    canvas.add(2, 15, "2. Scaled dot-product attention", color=96)
    body(canvas, 2, 19, "A(Q,K,V) =")
    body(canvas, 16, 19, "softmax")
    delimiters(canvas, 25, 41, 17, 7, round_brackets=True)
    body(canvas, 31, 17, "QK")
    script(canvas, 33, 17, "T", alignment=Align.TOP)
    body(canvas, 27, 19, "──────────────")
    canvas.add(33, 21, "───")
    body(canvas, 31, 22, "√")
    body(canvas, 33, 22, "d")
    script(canvas, 35, 23, "k")
    body(canvas, 44, 19, "V")


def matrix_values(variant):
    a = ((1, 2), (3, 4)) if variant == 0 else ((1, 0), (0, 1))
    b = ((5, 6), (7, 8))
    c = tuple(tuple(sum(a[i][k] * b[k][j] for k in range(2)) for j in range(2)) for i in range(2))
    return a, b, c


def matrix(canvas, variant):
    canvas.add(2, 27, "3. Matrix multiplication / indexed and numerical", color=96)
    body(canvas, 3, 30, "C")
    script(canvas, 5, 31, "ij")
    body(canvas, 9, 30, "=")
    canvas.add(16, 30, "Σ", scale=(2, 3, 4, Align.CENTER))
    script(canvas, 16, 29, "n")
    script(canvas, 15, 32, "k=1", (1, 2))
    body(canvas, 22, 30, "A")
    script(canvas, 24, 31, "ik")
    body(canvas, 29, 30, "B")
    script(canvas, 31, 31, "kj")
    for x, values in zip((4, 17, 32), matrix_values(variant)):
        width = max(len(str(value)) for row in values for value in row)
        first = "  ".join(f"{value:>{width}}" for value in values[0])
        last = "  ".join(f"{value:>{width}}" for value in values[1])
        canvas.add(x, 35, "⎛" + first + "⎞")
        canvas.add(x, 36, "⎜" + " " * len(first) + "⎟")
        canvas.add(x, 37, "⎝" + last + "⎠")
    canvas.add(13, 36, "×")
    canvas.add(28, 36, "=")


def paint(canvas, variant):
    rlhf(canvas)
    attention(canvas)
    matrix(canvas, variant)
