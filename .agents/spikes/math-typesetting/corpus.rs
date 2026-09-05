//! Fixed research corpus: inspect non-image cells and baselines; not a product test.
use std::time::Instant;
use tui_math::MathRenderer;

const CORPUS: &[(&str, &str)] = &[
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

fn main() {
    let renderer = MathRenderer::new();
    let context = katex::KatexContext::default();
    let settings = katex::Settings::builder()
        .display_mode(true)
        .output(katex::OutputFormat::Mathml)
        .build();
    for (name, source) in CORPUS {
        println!("\n{name}: {source}");
        let started = Instant::now();
        match renderer.render_to_box(source) {
            Ok(layout) => {
                println!(
                    "{}x{} cells, baseline={}, elapsed={}us\n{}",
                    layout.width,
                    layout.height,
                    layout.baseline,
                    started.elapsed().as_micros(),
                    layout.to_string()
                );
                if *name == "nested fraction" {
                    println!("partial scroll, original rows 2..4:");
                    for y in 2..layout.height.min(4) {
                        for x in 0..layout.width {
                            print!("{}", layout.get_grapheme(x, y));
                        }
                        println!();
                    }
                }
                for viewport_width in [120, 88, 60] {
                    if layout.width > viewport_width {
                        println!(
                            "width {viewport_width}: OVERFLOW {} cells",
                            layout.width - viewport_width
                        );
                    }
                }
            }
            Err(error) => println!("ERROR ({:?}): {error}", started.elapsed()),
        }
        match katex::render_to_string(&context, source, &settings) {
            Ok(mathml) => match renderer.render_mathml(&mathml) {
                Ok(cells) => println!("Rust KaTeX MathML → same cell renderer:\n{cells}"),
                Err(error) => println!("MathML CELL ERROR: {error}"),
            },
            Err(error) => println!("RUST KATEX ERROR: {error}"),
        }
    }
}
