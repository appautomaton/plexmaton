//! Separate performance lane: wall-clock measurements are reported, never asserted in tests.
use crate::support;
use plexmaton_math::Formula;
use std::{
    hint::black_box,
    io::{self, Write as _},
    time::{Duration, Instant},
};

fn sample(sources: &[&str]) -> Result<(Duration, Duration, usize), plexmaton_math::MathError> {
    let started = Instant::now();
    let formulas = sources
        .iter()
        .map(|source| Formula::parse(black_box(source)))
        .collect::<Result<Vec<_>, _>>()?;
    let prepare = started.elapsed();
    let started = Instant::now();
    let mut runs = 0;
    for width in [120, 88, 60] {
        for formula in &formulas {
            runs += black_box(formula.layout(width)?).runs().len();
        }
    }
    Ok((prepare, started.elapsed(), runs))
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = support::reply();
    let sources: Vec<_> = fixture
        .math
        .iter()
        .map(|span| &fixture.text[span.start..span.end])
        .collect();
    let cold = sample(&sources)?;
    let mut prepare = Vec::new();
    let mut project = Vec::new();
    for _ in 0..101 {
        let result = sample(&sources)?;
        assert_eq!(result.2, cold.2);
        prepare.push(result.0);
        project.push(result.1);
    }
    prepare.sort_unstable();
    project.sort_unstable();
    let mut output = io::BufWriter::new(io::stdout().lock());
    writeln!(
        output,
        "{} build; complete 61-formula reply; 183 projections at 120/88/60; {} native runs",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        cold.2
    )?;
    writeln!(
        output,
        "cold first batch: prepare {:.3} ms; project {:.3} ms",
        cold.0.as_secs_f64() * 1000.0,
        cold.1.as_secs_f64() * 1000.0
    )?;
    for (name, values) in [
        ("prepare 61 formulas", prepare),
        ("project 183 layouts", project),
    ] {
        writeln!(
            output,
            "101 warm batches / {name}: p50 {:.3} ms; p95 {:.3} ms; max {:.3} ms",
            values[50].as_secs_f64() * 1000.0,
            values[95].as_secs_f64() * 1000.0,
            values[100].as_secs_f64() * 1000.0
        )?;
    }
    output.flush()?;
    Ok(())
}
