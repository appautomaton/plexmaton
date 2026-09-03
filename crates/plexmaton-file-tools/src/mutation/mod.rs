//! Observation-bound exact replacements and create-if-absent publication.

mod compile;
mod publish;
mod types;

use std::{fs::File, io::Read as _};

pub(crate) use types::{
    ByteSplice, CanonicalEdit, CreateArguments, EditArguments, MAX_MUTATION_ARGUMENT_BYTES,
    MAX_MUTATION_EDITS, MAX_MUTATION_SOURCE_BYTES, MutationError,
};

use crate::{
    FileCancellation, WorkspaceRoot,
    observation::{FileVersion, ObservationStore},
};
use compile::{apply_splices, compile_edit, validate_canonical, validate_create};
use publish::{PublishHooks, publish_create, publish_replace};

pub(crate) fn admit_edit(
    root: &WorkspaceRoot,
    observations: &ObservationStore,
    arguments: &EditArguments,
    cancellation: &FileCancellation,
) -> Result<CanonicalEdit, MutationError> {
    compile_edit(root, observations, arguments, cancellation)
}

pub(crate) fn admit_create(
    root: &WorkspaceRoot,
    arguments: &CreateArguments,
    cancellation: &FileCancellation,
) -> Result<CreateArguments, MutationError> {
    admit_create_before_return(root, arguments, cancellation, || {})
}

fn admit_create_before_return(
    root: &WorkspaceRoot,
    arguments: &CreateArguments,
    cancellation: &FileCancellation,
    before_return: impl FnOnce(),
) -> Result<CreateArguments, MutationError> {
    if cancellation.is_cancelled() {
        return Err(MutationError::Cancelled);
    }
    validate_create(arguments)?;
    let target = root.mutation_path(&arguments.path)?;
    target.require_absent().map_err(|error| match error {
        crate::PathError::AlreadyExists => MutationError::CreateCollision,
        other => MutationError::Path(other),
    })?;
    let canonical = CreateArguments {
        path: target.display().to_owned(),
        content: arguments.content.clone(),
    };
    before_return();
    if cancellation.is_cancelled() {
        return Err(MutationError::Cancelled);
    }
    Ok(canonical)
}

pub(crate) fn execute_edit(
    root: &WorkspaceRoot,
    observations: &ObservationStore,
    mut canonical: CanonicalEdit,
    cancellation: &FileCancellation,
) -> Result<usize, MutationError> {
    execute_edit_with(
        root,
        observations,
        &mut canonical,
        cancellation,
        PublishHooks::default(),
    )
}

fn execute_edit_with(
    root: &WorkspaceRoot,
    observations: &ObservationStore,
    canonical: &mut CanonicalEdit,
    cancellation: &FileCancellation,
    hooks: PublishHooks<'_>,
) -> Result<usize, MutationError> {
    if cancellation.is_cancelled() {
        return Err(MutationError::Cancelled);
    }
    let observation_id = canonical.observation_id()?;
    let observed = observations
        .get(observation_id)
        .ok_or(MutationError::StaleObservation)?;
    if observed.path() != canonical.path {
        return Err(MutationError::StaleObservation);
    }
    validate_canonical(canonical, observed.byte_range().clone())?;
    let target = root
        .mutation_path(&canonical.path)
        .map_err(map_stale_path)?;
    let file = target.open_existing().map_err(map_stale_path)?;
    let mode = file_mode(&file)?;
    let (source, version) = read_source(file, cancellation)?;
    if &version != observed.version() || source.len() != canonical.source_len {
        return Err(MutationError::StaleObservation);
    }
    if canonical
        .splices
        .iter()
        .any(|splice| source.get(splice.start..splice.end) != Some(splice.expected.as_bytes()))
    {
        return Err(MutationError::StaleObservation);
    }
    let final_bytes = apply_splices(&source, &canonical.splices)?;
    publish_replace(
        &target,
        &source,
        &final_bytes,
        observed.version(),
        mode,
        cancellation,
        hooks,
    )?;
    Ok(canonical.splices.len())
}

pub(crate) fn execute_create(
    root: &WorkspaceRoot,
    arguments: &CreateArguments,
    cancellation: &FileCancellation,
) -> Result<usize, MutationError> {
    execute_create_with(root, arguments, cancellation, PublishHooks::default())
}

fn execute_create_with(
    root: &WorkspaceRoot,
    arguments: &CreateArguments,
    cancellation: &FileCancellation,
    hooks: PublishHooks<'_>,
) -> Result<usize, MutationError> {
    if cancellation.is_cancelled() {
        return Err(MutationError::Cancelled);
    }
    validate_create(arguments)?;
    let target = root.mutation_path(&arguments.path)?;
    publish_create(&target, arguments.content.as_bytes(), cancellation, hooks)?;
    Ok(arguments.content.len())
}

pub(super) fn read_source(
    file: File,
    cancellation: &FileCancellation,
) -> Result<(Vec<u8>, FileVersion), MutationError> {
    let before = FileVersion::read(&file).map_err(|error| MutationError::Io(error.kind()))?;
    if before.len() > u64::try_from(MAX_MUTATION_SOURCE_BYTES).unwrap_or(u64::MAX) {
        return Err(MutationError::SourceTooLarge);
    }
    let mut reader = file;
    let mut bytes = Vec::with_capacity(usize::try_from(before.len()).unwrap_or(0));
    let mut buffer = [0_u8; 8192];
    loop {
        if cancellation.is_cancelled() {
            return Err(MutationError::Cancelled);
        }
        let remaining = MAX_MUTATION_SOURCE_BYTES.saturating_sub(bytes.len());
        let allowance = remaining.saturating_add(1).min(buffer.len());
        let read = reader
            .read(&mut buffer[..allowance])
            .map_err(|error| MutationError::Io(error.kind()))?;
        if read == 0 {
            break;
        }
        if read > remaining {
            return Err(MutationError::SourceTooLarge);
        }
        if buffer[..read].contains(&0) {
            return Err(MutationError::Binary);
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    std::str::from_utf8(&bytes).map_err(|_| MutationError::InvalidUtf8)?;
    let after = FileVersion::read(&reader).map_err(|error| MutationError::Io(error.kind()))?;
    if before != after {
        return Err(MutationError::ChangedBeforeCommit);
    }
    Ok((bytes, after))
}

#[cfg(unix)]
fn file_mode(file: &File) -> Result<u16, MutationError> {
    use std::os::unix::fs::MetadataExt as _;

    file.metadata()
        .and_then(|metadata| {
            u16::try_from(metadata.mode() & 0o777)
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))
        })
        .map_err(|error| MutationError::Io(error.kind()))
}

fn map_stale_path(error: crate::PathError) -> MutationError {
    match error {
        crate::PathError::NotFound
        | crate::PathError::AlreadyExists
        | crate::PathError::Symlink
        | crate::PathError::NotFile => MutationError::StaleObservation,
        other => MutationError::Path(other),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use crate::{FileTools, ReadRequest};

    use super::compile::compile_edit_before_return;
    use super::publish::WRITE_CHUNK_BYTES;
    use super::types::ExactEdit;
    use super::{
        CreateArguments, EditArguments, FileCancellation, MutationError, PublishHooks,
        admit_create, admit_create_before_return, admit_edit, execute_create_with,
        execute_edit_with,
    };

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn fixture(name: &str) -> PathBuf {
        let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "plexmaton-mutation-{name}-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir(&directory).unwrap_or_else(|error| panic!("create fixture: {error}"));
        directory
    }

    fn tools(directory: &Path) -> FileTools {
        FileTools::open(directory, "/bin/false", "/bin/false")
            .unwrap_or_else(|error| panic!("open tools: {error}"))
    }

    fn edit(tools: &mut FileTools, path: &str, old: &str, new: &str) -> super::CanonicalEdit {
        let request = ReadRequest::new(path.to_owned(), None, None)
            .unwrap_or_else(|error| panic!("read request: {error}"));
        let observation = tools
            .read(&request, &FileCancellation::new())
            .unwrap_or_else(|error| panic!("read fixture: {error}"))
            .observation
            .as_token();
        admit_edit(
            &tools.root,
            &tools.observations,
            &EditArguments {
                path: path.to_owned(),
                observation,
                edits: vec![ExactEdit {
                    old_text: old.to_owned(),
                    new_text: new.to_owned(),
                }],
            },
            &FileCancellation::new(),
        )
        .unwrap_or_else(|error| panic!("admit edit: {error}"))
    }

    fn staging_files(directory: &Path) -> Vec<PathBuf> {
        fs::read_dir(directory)
            .unwrap_or_else(|error| panic!("list fixture: {error}"))
            .map(|entry| {
                entry
                    .unwrap_or_else(|error| panic!("read directory entry: {error}"))
                    .path()
            })
            .filter(|path| {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                name.starts_with(".plexmaton-") && name.ends_with(".tmp")
            })
            .collect()
    }

    fn assert_no_staging_files(directory: &Path) {
        let paths = staging_files(directory);
        assert!(paths.is_empty(), "staging residue: {paths:?}");
    }

    /// MUT-1/MUT-6: a cancellation sampled at the admission publication edge wins for both
    /// mutation forms.
    #[test]
    fn admission_never_returns_a_trusted_call_after_final_cancellation() {
        let directory = fixture("admission-cancel");
        fs::write(directory.join("file"), b"old\n")
            .unwrap_or_else(|error| panic!("write fixture: {error}"));
        let mut tools = tools(&directory);
        let request = ReadRequest::new("file".to_owned(), None, None)
            .unwrap_or_else(|error| panic!("read request: {error}"));
        let observation = tools
            .read(&request, &FileCancellation::new())
            .unwrap_or_else(|error| panic!("read fixture: {error}"))
            .observation
            .as_token();
        let edit = EditArguments {
            path: "file".to_owned(),
            observation,
            edits: vec![ExactEdit {
                old_text: "old".to_owned(),
                new_text: "new".to_owned(),
            }],
        };
        let cancellation = FileCancellation::new();
        let to_cancel = cancellation.clone();
        assert_eq!(
            compile_edit_before_return(
                &tools.root,
                &tools.observations,
                &edit,
                &cancellation,
                || to_cancel.cancel(),
            ),
            Err(MutationError::Cancelled)
        );

        let create = CreateArguments {
            path: "new".to_owned(),
            content: "content\n".to_owned(),
        };
        let cancellation = FileCancellation::new();
        let to_cancel = cancellation.clone();
        assert_eq!(
            admit_create_before_return(&tools.root, &create, &cancellation, || to_cancel.cancel()),
            Err(MutationError::Cancelled)
        );
        assert!(!directory.join("new").exists());
        fs::remove_dir_all(directory).unwrap_or_else(|error| panic!("remove fixture: {error}"));
    }

    /// MUT-4/MUT-6: staging write failure and pre-publication cancellation preserve the original.
    #[test]
    fn replacement_faults_leave_no_partial_target_or_staging_file() {
        let directory = fixture("replace-faults");
        fs::write(directory.join("file"), b"old\n")
            .unwrap_or_else(|error| panic!("write fixture: {error}"));
        let mut tools = tools(&directory);
        let canonical = edit(&mut tools, "file", "old", "replacement");
        assert!(matches!(
            execute_edit_with(
                &tools.root,
                &tools.observations,
                &mut canonical.clone(),
                &FileCancellation::new(),
                PublishHooks {
                    fail_write_after: Some(1),
                    before_publish: None,
                    before_commit: None,
                    after_write_chunk: None,
                },
            ),
            Err(MutationError::Io(_))
        ));
        assert_eq!(
            fs::read(directory.join("file"))
                .unwrap_or_else(|error| panic!("read original: {error}")),
            b"old\n"
        );
        assert_no_staging_files(&directory);

        let cancellation = FileCancellation::new();
        let to_cancel = cancellation.clone();
        let cancel = || to_cancel.cancel();
        assert_eq!(
            execute_edit_with(
                &tools.root,
                &tools.observations,
                &mut canonical.clone(),
                &cancellation,
                PublishHooks {
                    fail_write_after: None,
                    before_publish: Some(&cancel),
                    before_commit: None,
                    after_write_chunk: None,
                },
            ),
            Err(MutationError::Cancelled)
        );
        assert_eq!(
            fs::read(directory.join("file"))
                .unwrap_or_else(|error| panic!("read cancelled original: {error}")),
            b"old\n"
        );
        assert_no_staging_files(&directory);

        let chunked = edit(&mut tools, "file", "old", &"x".repeat(20_000));
        let chunk_cancellation = FileCancellation::new();
        let to_cancel = chunk_cancellation.clone();
        assert_eq!(WRITE_CHUNK_BYTES, 8192);
        let chunks = Cell::new(0_usize);
        let cancel_after_chunk = |written: usize| {
            chunks.set(chunks.get().saturating_add(1));
            if written >= WRITE_CHUNK_BYTES {
                to_cancel.cancel();
            }
        };
        assert_eq!(
            execute_edit_with(
                &tools.root,
                &tools.observations,
                &mut chunked.clone(),
                &chunk_cancellation,
                PublishHooks {
                    fail_write_after: None,
                    before_publish: None,
                    before_commit: None,
                    after_write_chunk: Some(&cancel_after_chunk),
                },
            ),
            Err(MutationError::Cancelled)
        );
        assert_eq!(chunks.get(), 1);
        assert_eq!(
            fs::read(directory.join("file"))
                .unwrap_or_else(|error| panic!("read chunk-cancel original: {error}")),
            b"old\n"
        );
        assert_no_staging_files(&directory);

        let final_cancellation = FileCancellation::new();
        let to_cancel = final_cancellation.clone();
        let cancel_after_validation = || to_cancel.cancel();
        assert_eq!(
            execute_edit_with(
                &tools.root,
                &tools.observations,
                &mut canonical.clone(),
                &final_cancellation,
                PublishHooks {
                    fail_write_after: None,
                    before_publish: None,
                    before_commit: Some(&cancel_after_validation),
                    after_write_chunk: None,
                },
            ),
            Err(MutationError::Cancelled)
        );
        assert_eq!(
            fs::read(directory.join("file"))
                .unwrap_or_else(|error| panic!("read final-cancel original: {error}")),
            b"old\n"
        );
        assert_no_staging_files(&directory);
        fs::remove_dir_all(directory).unwrap_or_else(|error| panic!("remove fixture: {error}"));
    }

    /// MUT-3: a writer arriving immediately before the final descriptor recheck is retained.
    #[test]
    fn writer_before_replace_publication_is_preserved() {
        let directory = fixture("replace-writer");
        let path = directory.join("file");
        fs::write(&path, b"old\n").unwrap_or_else(|error| panic!("write fixture: {error}"));
        let mut tools = tools(&directory);
        let mut canonical = edit(&mut tools, "file", "old", "new");
        let write = || {
            fs::write(&path, b"operator\n")
                .unwrap_or_else(|error| panic!("write concurrent value: {error}"));
        };

        assert_eq!(
            execute_edit_with(
                &tools.root,
                &tools.observations,
                &mut canonical,
                &FileCancellation::new(),
                PublishHooks {
                    fail_write_after: None,
                    before_publish: Some(&write),
                    before_commit: None,
                    after_write_chunk: None,
                },
            ),
            Err(MutationError::ChangedBeforeCommit)
        );
        assert_eq!(
            fs::read(&path).unwrap_or_else(|error| panic!("read concurrent value: {error}")),
            b"operator\n"
        );
        assert_no_staging_files(&directory);
        fs::remove_dir_all(directory).unwrap_or_else(|error| panic!("remove fixture: {error}"));
    }

    /// MUT-4: publication never trusts a staging name after another writer replaces its entry.
    #[test]
    fn replaced_or_modified_staging_entry_is_never_published_or_wrongly_deleted() {
        let directory = fixture("replace-staging");
        let path = directory.join("file");
        fs::write(&path, b"old\n").unwrap_or_else(|error| panic!("write fixture: {error}"));
        let mut tools = tools(&directory);
        let mut canonical = edit(&mut tools, "file", "old", "new");
        let replace_staging = || {
            let staged = staging_files(&directory);
            assert_eq!(staged.len(), 1);
            fs::remove_file(&staged[0])
                .unwrap_or_else(|error| panic!("unlink owned staging entry: {error}"));
            fs::write(&staged[0], b"attacker\n")
                .unwrap_or_else(|error| panic!("replace staging entry: {error}"));
        };

        assert_eq!(
            execute_edit_with(
                &tools.root,
                &tools.observations,
                &mut canonical,
                &FileCancellation::new(),
                PublishHooks {
                    fail_write_after: None,
                    before_publish: Some(&replace_staging),
                    before_commit: None,
                    after_write_chunk: None,
                },
            ),
            Err(MutationError::StagingChanged)
        );
        assert_eq!(
            fs::read(&path).unwrap_or_else(|error| panic!("read original: {error}")),
            b"old\n"
        );
        let staged = staging_files(&directory);
        assert_eq!(staged.len(), 1);
        assert_eq!(
            fs::read(&staged[0]).unwrap_or_else(|error| panic!("read foreign staging: {error}")),
            b"attacker\n"
        );
        fs::remove_file(&staged[0])
            .unwrap_or_else(|error| panic!("remove foreign staging: {error}"));

        let tamper_in_place = || {
            let staged = staging_files(&directory);
            assert_eq!(staged.len(), 1);
            fs::write(&staged[0], b"same inode attacker\n")
                .unwrap_or_else(|error| panic!("tamper staging in place: {error}"));
        };
        assert_eq!(
            execute_edit_with(
                &tools.root,
                &tools.observations,
                &mut canonical.clone(),
                &FileCancellation::new(),
                PublishHooks {
                    fail_write_after: None,
                    before_publish: Some(&tamper_in_place),
                    before_commit: None,
                    after_write_chunk: None,
                },
            ),
            Err(MutationError::StagingChanged)
        );
        assert_eq!(
            fs::read(&path).unwrap_or_else(|error| panic!("read original: {error}")),
            b"old\n"
        );
        assert_no_staging_files(&directory);
        fs::remove_dir_all(directory).unwrap_or_else(|error| panic!("remove fixture: {error}"));
    }

    /// MUT-3/MUT-4: a leaf swapped to a symlink is never followed and outside bytes are untouched.
    #[test]
    fn symlink_swap_before_replace_is_refused() {
        let directory = fixture("replace-symlink");
        let path = directory.join("file");
        let outside = directory.with_extension("outside");
        fs::write(&path, b"old\n").unwrap_or_else(|error| panic!("write fixture: {error}"));
        fs::write(&outside, b"outside\n")
            .unwrap_or_else(|error| panic!("write outside fixture: {error}"));
        let mut tools = tools(&directory);
        let mut canonical = edit(&mut tools, "file", "old", "new");
        let swap = || {
            fs::remove_file(&path).unwrap_or_else(|error| panic!("remove target: {error}"));
            std::os::unix::fs::symlink(&outside, &path)
                .unwrap_or_else(|error| panic!("install symlink: {error}"));
        };

        assert!(matches!(
            execute_edit_with(
                &tools.root,
                &tools.observations,
                &mut canonical,
                &FileCancellation::new(),
                PublishHooks {
                    fail_write_after: None,
                    before_publish: Some(&swap),
                    before_commit: None,
                    after_write_chunk: None,
                },
            ),
            Err(MutationError::ChangedBeforeCommit)
        ));
        assert_eq!(
            fs::read(&outside).unwrap_or_else(|error| panic!("read outside: {error}")),
            b"outside\n"
        );
        assert_no_staging_files(&directory);
        fs::remove_dir_all(directory).unwrap_or_else(|error| panic!("remove fixture: {error}"));
        fs::remove_file(outside).unwrap_or_else(|error| panic!("remove outside: {error}"));
    }

    /// MUT-5/MUT-6: create faults leave absence intact, while a concurrent creator is preserved.
    #[test]
    fn create_faults_and_collision_never_publish_staging_bytes() {
        let directory = fixture("create-faults");
        let tools = tools(&directory);
        let arguments = CreateArguments {
            path: "new".to_owned(),
            content: "generated\n".to_owned(),
        };
        let admitted = admit_create(&tools.root, &arguments, &FileCancellation::new())
            .unwrap_or_else(|error| panic!("admit create: {error}"));
        assert!(matches!(
            execute_create_with(
                &tools.root,
                &admitted,
                &FileCancellation::new(),
                PublishHooks {
                    fail_write_after: Some(1),
                    before_publish: None,
                    before_commit: None,
                    after_write_chunk: None,
                },
            ),
            Err(MutationError::Io(_))
        ));
        assert!(!directory.join("new").exists());
        assert_no_staging_files(&directory);

        let cancellation = FileCancellation::new();
        let to_cancel = cancellation.clone();
        let cancel = || to_cancel.cancel();
        assert_eq!(
            execute_create_with(
                &tools.root,
                &admitted,
                &cancellation,
                PublishHooks {
                    fail_write_after: None,
                    before_publish: Some(&cancel),
                    before_commit: None,
                    after_write_chunk: None,
                },
            ),
            Err(MutationError::Cancelled)
        );
        assert!(!directory.join("new").exists());
        assert_no_staging_files(&directory);

        let final_cancellation = FileCancellation::new();
        let to_cancel = final_cancellation.clone();
        let cancel_after_validation = || to_cancel.cancel();
        assert_eq!(
            execute_create_with(
                &tools.root,
                &admitted,
                &final_cancellation,
                PublishHooks {
                    fail_write_after: None,
                    before_publish: None,
                    before_commit: Some(&cancel_after_validation),
                    after_write_chunk: None,
                },
            ),
            Err(MutationError::Cancelled)
        );
        assert!(!directory.join("new").exists());
        assert_no_staging_files(&directory);

        let path = directory.join("new");
        let create = || {
            fs::write(&path, b"operator\n")
                .unwrap_or_else(|error| panic!("concurrent create: {error}"));
        };
        assert_eq!(
            execute_create_with(
                &tools.root,
                &admitted,
                &FileCancellation::new(),
                PublishHooks {
                    fail_write_after: None,
                    before_publish: Some(&create),
                    before_commit: None,
                    after_write_chunk: None,
                },
            ),
            Err(MutationError::CreateCollision)
        );
        assert_eq!(
            fs::read(&path).unwrap_or_else(|error| panic!("read concurrent create: {error}")),
            b"operator\n"
        );
        assert_no_staging_files(&directory);
        fs::remove_dir_all(directory).unwrap_or_else(|error| panic!("remove fixture: {error}"));
    }
}
