//! Fixed source-comparison and user-reported formulas, shared by tests and visual preview.
#[derive(serde::Deserialize)]
pub struct Reply {
    pub text: String,
    pub math: Vec<Span>,
}

#[derive(serde::Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub display: bool,
}

pub fn reply() -> Reply {
    serde_json::from_str(include_str!("../fixtures/attention-derivatives.json"))
        .expect("fixed corpus")
}

pub fn logits_reply() -> Reply {
    serde_json::from_str(include_str!("../fixtures/logits.json")).expect("user logits corpus")
}

pub const CORPUS: &[(&str, &str)] = &[
    ("nested fraction", r"\frac{1}{1+\frac{a}{b}}"),
    ("nested root", r"\sqrt{1+\sqrt{x^2+y^2}}"),
    ("scripts", r"x_{ij}^{n+1}+\alpha_2"),
    ("matrix", r"\begin{pmatrix}a&b\\c&d\end{pmatrix}"),
    ("cases", r"f(x)=\begin{cases}x^2&x\geq0\\-x&x<0\end{cases}"),
    ("aligned", r"\begin{aligned}a&=b+c\\&=d+e\end{aligned}"),
    (
        "wide",
        r"a_1+a_2+a_3+a_4+a_5+a_6+a_7+a_8+a_9+a_{10}+a_{11}+a_{12}+a_{13}+a_{14}+a_{15}+a_{16}+a_{17}+a_{18}+a_{19}+a_{20}=\frac{x}{y}",
    ),
    ("incomplete stream", r"\frac{1}{1+\sqrt{"),
];
