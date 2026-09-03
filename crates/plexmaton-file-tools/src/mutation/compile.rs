use std::ops::Range;

use crate::{FileCancellation, ObservationId, WorkspaceRoot, observation::ObservationStore};

use super::{
    ByteSplice, CanonicalEdit, CreateArguments, EditArguments, MAX_MUTATION_ARGUMENT_BYTES,
    MAX_MUTATION_EDITS, MAX_MUTATION_SOURCE_BYTES, MutationError, map_stale_path, read_source,
};

pub(super) fn compile_edit(
    root: &WorkspaceRoot,
    observations: &ObservationStore,
    arguments: &EditArguments,
    cancellation: &FileCancellation,
) -> Result<CanonicalEdit, MutationError> {
    compile_edit_before_return(root, observations, arguments, cancellation, || {})
}

pub(super) fn compile_edit_before_return(
    root: &WorkspaceRoot,
    observations: &ObservationStore,
    arguments: &EditArguments,
    cancellation: &FileCancellation,
    before_return: impl FnOnce(),
) -> Result<CanonicalEdit, MutationError> {
    if cancellation.is_cancelled() {
        return Err(MutationError::Cancelled);
    }
    validate_edit_arguments(arguments)?;
    let observation_id =
        ObservationId::from_token(&arguments.observation).ok_or(MutationError::StaleObservation)?;
    let observed = observations
        .get(observation_id)
        .ok_or(MutationError::StaleObservation)?;
    let target = root
        .mutation_path(&arguments.path)
        .map_err(map_stale_path)?;
    if observed.path() != target.display() {
        return Err(MutationError::StaleObservation);
    }
    let file = target.open_existing().map_err(map_stale_path)?;
    let (source, version) = read_source(file, cancellation)?;
    if cancellation.is_cancelled() {
        return Err(MutationError::Cancelled);
    }
    if &version != observed.version() {
        return Err(MutationError::StaleObservation);
    }
    let source_text = std::str::from_utf8(&source).map_err(|_| MutationError::InvalidUtf8)?;
    let observed_text = source_text
        .get(observed.byte_range().clone())
        .ok_or(MutationError::StaleObservation)?;
    let mut splices = Vec::with_capacity(arguments.edits.len());
    for edit in &arguments.edits {
        let positions = find_positions(observed_text, &edit.old_text, observed.byte_range().start);
        let [start] = positions.as_slice() else {
            return Err(if positions.is_empty() {
                MutationError::SourceMismatch
            } else {
                MutationError::AmbiguousTarget
            });
        };
        let span = *start..start.saturating_add(edit.old_text.len());
        if !observed.contains(span.clone()) {
            return Err(MutationError::SourceMismatch);
        }
        splices.push(ByteSplice {
            start: span.start,
            end: span.end,
            expected: edit.old_text.clone(),
            replacement: edit.new_text.clone(),
        });
    }
    validate_splices(&mut splices, source.len(), observed.byte_range().clone())?;
    let final_len = resulting_len(source.len(), &splices)?;
    if final_len > MAX_MUTATION_SOURCE_BYTES {
        return Err(MutationError::ResultTooLarge);
    }
    let canonical = CanonicalEdit {
        path: target.display().to_owned(),
        observation: arguments.observation.clone(),
        source_len: source.len(),
        splices,
    };
    before_return();
    if cancellation.is_cancelled() {
        return Err(MutationError::Cancelled);
    }
    Ok(canonical)
}

pub(super) fn validate_create(arguments: &CreateArguments) -> Result<(), MutationError> {
    if arguments.content.len() > MAX_MUTATION_ARGUMENT_BYTES {
        return Err(MutationError::InvalidArguments);
    }
    if arguments.content.as_bytes().contains(&0) {
        return Err(MutationError::Binary);
    }
    Ok(())
}

pub(super) fn validate_canonical(
    canonical: &mut CanonicalEdit,
    observed_window: Range<usize>,
) -> Result<(), MutationError> {
    if canonical.path.is_empty()
        || canonical.source_len > MAX_MUTATION_SOURCE_BYTES
        || canonical.splices.is_empty()
        || canonical.splices.len() > MAX_MUTATION_EDITS
        || canonical.splices.iter().fold(0_usize, |total, splice| {
            total
                .saturating_add(splice.expected.len())
                .saturating_add(splice.replacement.len())
        }) > MAX_MUTATION_ARGUMENT_BYTES
    {
        return Err(MutationError::InvalidArguments);
    }
    validate_splices(
        &mut canonical.splices,
        canonical.source_len,
        observed_window,
    )?;
    if resulting_len(canonical.source_len, &canonical.splices)? > MAX_MUTATION_SOURCE_BYTES {
        return Err(MutationError::ResultTooLarge);
    }
    Ok(())
}

pub(super) fn apply_splices(
    source: &[u8],
    splices: &[ByteSplice],
) -> Result<Vec<u8>, MutationError> {
    let capacity = resulting_len(source.len(), splices)?;
    let mut result = Vec::with_capacity(capacity);
    let mut cursor = 0;
    for splice in splices {
        result.extend_from_slice(&source[cursor..splice.start]);
        result.extend_from_slice(splice.replacement.as_bytes());
        cursor = splice.end;
    }
    result.extend_from_slice(&source[cursor..]);
    Ok(result)
}

fn validate_edit_arguments(arguments: &EditArguments) -> Result<(), MutationError> {
    if arguments.path.is_empty()
        || arguments.edits.is_empty()
        || arguments.edits.len() > MAX_MUTATION_EDITS
        || arguments.edits.iter().any(|edit| {
            edit.old_text.is_empty()
                || edit.old_text == edit.new_text
                || edit.old_text.as_bytes().contains(&0)
                || edit.new_text.as_bytes().contains(&0)
                || edit.old_text.len().saturating_add(edit.new_text.len())
                    > MAX_MUTATION_ARGUMENT_BYTES
        })
        || arguments.edits.iter().fold(0_usize, |total, edit| {
            total
                .saturating_add(edit.old_text.len())
                .saturating_add(edit.new_text.len())
        }) > MAX_MUTATION_ARGUMENT_BYTES
    {
        return Err(MutationError::InvalidArguments);
    }
    Ok(())
}

fn validate_splices(
    splices: &mut [ByteSplice],
    source_len: usize,
    observed_window: Range<usize>,
) -> Result<(), MutationError> {
    splices.sort_by_key(|splice| (splice.start, splice.end));
    for splice in splices.iter() {
        if splice.start >= splice.end
            || splice.end > source_len
            || splice.end.saturating_sub(splice.start) != splice.expected.len()
            || splice.expected.is_empty()
            || splice.expected == splice.replacement
            || splice.expected.as_bytes().contains(&0)
            || splice.replacement.as_bytes().contains(&0)
            || splice.start < observed_window.start
            || splice.end > observed_window.end
        {
            return Err(MutationError::InvalidArguments);
        }
    }
    if splices.windows(2).any(|pair| pair[0].end > pair[1].start) {
        return Err(MutationError::OverlappingEdits);
    }
    Ok(())
}

fn find_positions(haystack: &str, needle: &str, base: usize) -> Vec<usize> {
    let mut positions = Vec::with_capacity(2);
    let mut cursor = 0;
    while cursor < haystack.len() {
        let Some(relative) = haystack[cursor..].find(needle) else {
            break;
        };
        let position = cursor.saturating_add(relative);
        positions.push(base.saturating_add(position));
        if positions.len() == 2 {
            break;
        }
        let step = haystack[position..]
            .chars()
            .next()
            .map_or(1, char::len_utf8);
        cursor = position.saturating_add(step);
    }
    positions
}

fn resulting_len(source_len: usize, splices: &[ByteSplice]) -> Result<usize, MutationError> {
    splices.iter().try_fold(source_len, |length, splice| {
        length
            .checked_sub(splice.end.saturating_sub(splice.start))
            .and_then(|value| value.checked_add(splice.replacement.len()))
            .ok_or(MutationError::ResultTooLarge)
    })
}
