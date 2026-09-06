//! Finite checkpoint counterexamples for the current journal budget walk.
//!
//! `path_span_prefix` reproduces the boundary selection in
//! crates/plexmaton-agent/src/journal/budget.rs, not a production checkpoint API.
//! Checkpoints do not exist there yet. All requests here share one environment.
//! No storage, provider tokenization, or cache-hit behavior is modeled.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Atom {
    Original(u8),
    Summary(u8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Epoch {
    Original,
    Checkpoint(u8),
}

struct Measurement {
    epoch: Epoch,
    input: Vec<Atom>,
    journal_boundary: usize,
}

// Existing pre-checkpoint algorithm; `partition_point` requires monotonic spans.
fn path_span_prefix(spans: &[(usize, usize)], boundary: usize) -> Option<usize> {
    let count = spans.partition_point(|&(_, end)| end <= boundary);
    if spans
        .get(count)
        .is_some_and(|&(start, _)| start <= boundary)
    {
        None
    } else {
        Some(count)
    }
}

// Independent BUD-2 oracle for this finite vocabulary. A production design can
// index/check identities without storing a second copy of each request.
fn exact_prefix(epoch: Epoch, input: &[Atom], measurement: &Measurement) -> Option<usize> {
    (epoch == measurement.epoch && input.starts_with(&measurement.input))
        .then_some(measurement.input.len())
}

#[test]
fn unchanged_path_accepts_a_measured_prefix_and_new_suffix() {
    let measured = Measurement {
        epoch: Epoch::Original,
        input: vec![Atom::Original(0), Atom::Original(1)],
        journal_boundary: 1,
    };
    let input = [Atom::Original(0), Atom::Original(1), Atom::Original(2)];
    assert_eq!(path_span_prefix(&[(0, 0), (1, 1), (2, 2)], 1), Some(2));
    assert_eq!(exact_prefix(Epoch::Original, &input, &measured), Some(2));
}

#[test]
fn checkpoint_before_retained_suffix_breaks_the_path_span_assumption() {
    let measured = Measurement {
        epoch: Epoch::Original,
        input: (0..4).map(Atom::Original).collect(),
        journal_boundary: 3,
    };
    // The checkpoint is appended at journal position 4 but projects before
    // retained entries 2 and 3, preserving those entries' original identities.
    let replacement = [Atom::Summary(0), Atom::Original(2), Atom::Original(3)];
    let spans = [(4, 4), (2, 2), (3, 3)];
    assert!(!spans.windows(2).all(|pair| pair[0].1 <= pair[1].1));
    // This witness records the current partition algorithm's false full anchor.
    assert_eq!(path_span_prefix(&spans, measured.journal_boundary), Some(3));
    assert_eq!(
        exact_prefix(Epoch::Checkpoint(0), &replacement, &measured),
        None
    );
}

#[test]
fn a_covered_request_cannot_measure_only_the_new_environment() {
    let measured = Measurement {
        epoch: Epoch::Original,
        input: vec![Atom::Original(0), Atom::Original(1)],
        journal_boundary: 1,
    };
    // No retained suffix is needed for a false anchor: count zero would treat
    // the old conversation's measured input as environment-only occupancy.
    assert_eq!(
        path_span_prefix(&[(2, 2)], measured.journal_boundary),
        Some(0)
    );
    assert_eq!(
        exact_prefix(Epoch::Checkpoint(0), &[Atom::Summary(0)], &measured),
        None
    );
}

#[test]
fn checkpoint_measurement_accepts_only_its_epoch_and_ordered_prefix() {
    let measured = Measurement {
        epoch: Epoch::Checkpoint(0),
        input: vec![Atom::Summary(0), Atom::Original(2)],
        journal_boundary: 4,
    };
    let extended = [Atom::Summary(0), Atom::Original(2), Atom::Original(3)];
    assert_eq!(
        exact_prefix(Epoch::Checkpoint(0), &extended, &measured),
        Some(2)
    );
    let reordered = [Atom::Summary(0), Atom::Original(3), Atom::Original(2)];
    assert_eq!(
        exact_prefix(Epoch::Checkpoint(0), &reordered, &measured),
        None
    );
    assert_eq!(
        exact_prefix(Epoch::Checkpoint(1), &extended, &measured),
        None
    );
}
