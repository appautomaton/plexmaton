//! Report the complete reply and structural corpus through the production math boundary.
use plexmaton_math::{Formula, MathMode};
use std::io::{self, Write as _};

#[path = "support/measure.rs"]
mod measure;
#[path = "../src/test_support.rs"]
mod support;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() == Some("--measure") {
        return measure::run();
    }
    let mut output = io::BufWriter::new(io::stdout().lock());
    let fixture = support::reply();
    for (index, span) in fixture.math.iter().enumerate() {
        report(
            &mut output,
            &format!("reply {index}"),
            &fixture.text[span.start..span.end],
            span.display,
        )?;
    }
    for (name, body) in support::CORPUS {
        report(&mut output, name, &format!("\\[{body}\\]"), true)?;
    }
    output.flush()?;
    Ok(())
}

fn report(output: &mut impl io::Write, name: &str, source: &str, display: bool) -> io::Result<()> {
    match Formula::parse(source) {
        Ok(formula) => {
            assert_eq!(formula.mode() == MathMode::Display, display);
            for width in [120, 88, 60] {
                match formula.layout(width) {
                    Ok(layout) => writeln!(
                        output,
                        "{name} @ {width}: {}x{}, axis {}",
                        layout.width(),
                        layout.height(),
                        layout.axis()
                    )?,
                    Err(error) => writeln!(output, "{name} @ {width}: ERROR {error}: {source}")?,
                }
            }
        }
        Err(error) => writeln!(output, "{name}: ERROR {error}: {source}")?,
    }
    Ok(())
}
