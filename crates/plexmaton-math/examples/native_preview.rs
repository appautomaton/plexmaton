//! Source-linked review exports for the real Kitty viewer; never an application image transport.
use plexmaton_math::{Formula, MathMode};
use std::io::{self, Write as _};
use std::path::Path;

#[path = "../src/test_support.rs"]
mod corpus;
#[path = "support/reply.rs"]
mod reply;
#[path = "support/review_svg.rs"]
mod review_svg;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args()
        .nth(1)
        .ok_or("provide an output directory")?;
    let directory = Path::new(&directory);
    let mut output = io::BufWriter::new(io::stdout().lock());
    std::fs::create_dir_all(directory)?;
    let fixture = if std::env::args().nth(2).as_deref() == Some("--logits") {
        corpus::logits_reply()
    } else {
        corpus::reply()
    };
    let formulas = fixture
        .math
        .iter()
        .map(|span| {
            let formula = Formula::parse(&fixture.text[span.start..span.end])?;
            assert_eq!(formula.mode() == MathMode::Display, span.display);
            Ok(formula)
        })
        .collect::<Result<Vec<_>, plexmaton_math::MathError>>()?;
    for width in [120, 88, 60] {
        let document = reply::document(&fixture, &formulas, width)?;
        std::fs::write(
            directory.join(format!("reply-{width}.json")),
            serde_json::to_vec(&document)?,
        )?;
        for (page, bounds) in document.pages.iter().enumerate() {
            std::fs::write(
                directory.join(format!("reply-{width}-{}.svg", page + 1)),
                review_svg::svg(&document, bounds),
            )?;
        }
        writeln!(
            output,
            "{width}: {} formulas, {} retained rows, {} complete-formula pages",
            document.formulas.len(),
            document.height,
            document.pages.len()
        )?;
    }
    // Keep the small structural corpus in the same source-linked export, including explicit refusal.
    let structural: Vec<_> = corpus::CORPUS.iter().map(|(name, body)| {
        let source = format!("\\[{body}\\]");
        let result = Formula::parse(&source).and_then(|formula| formula.layout(60));
        let result = match result {
            Ok(layout) => serde_json::json!({"width":layout.width(), "height":layout.height(), "axis":layout.axis(), "runs":layout.runs()}),
            Err(error) => serde_json::json!({"refusal":error.to_string()}),
        };
        serde_json::json!({"name": name, "source":source, "result":result})
    }).collect();
    std::fs::write(
        directory.join("structures.json"),
        serde_json::to_vec(&structural)?,
    )?;
    output.flush()?;
    Ok(())
}
