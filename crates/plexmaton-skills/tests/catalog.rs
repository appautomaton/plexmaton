use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use plexmaton_file_tools::{BoundedReadError, FileCancellation, PathError};
use plexmaton_skills::{
    MAX_FRONTMATTER_BYTES, MAX_SKILL_CANDIDATES, MAX_SKILL_CATALOG_BYTES, MAX_SKILL_CONTENT_BYTES,
    SkillCatalog, SkillDiagnosticKind, SkillError, SkillInvocation, SkillMetadataError,
    SkillMetadataField, SkillOrigin,
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "plexmaton-skills-{label}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap_or_else(|error| panic!("create fixture root: {error}"));
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, relative: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("create fixture parent: {error}"));
        }
        fs::write(path, bytes).unwrap_or_else(|error| panic!("write fixture: {error}"));
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn skill(name: &str, description: &str, fields: &str, body: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n{fields}---\n{body}")
}

fn discover(fixture: &TestDir) -> SkillCatalog {
    SkillCatalog::discover(
        &fixture.path().join("home"),
        &fixture.path().join("project"),
        &FileCancellation::new(),
    )
    .unwrap_or_else(|error| panic!("discover catalog: {error}"))
}

/// SKL-2/SKL-3: fixed precedence selects and pins one origin; reads never fall back.
#[test]
fn collisions_select_one_origin_and_missing_winners_do_not_fall_back() {
    let fixture = TestDir::new("collision");
    fixture.write(
        "project/.plexmaton/skills/shared/SKILL.md",
        skill("shared", "native", "", "\nnative\r\n"),
    );
    fixture.write(
        "project/.agents/skills/shared/SKILL.md",
        skill("shared", "portable", "", "\nportable\n"),
    );
    fixture.write(
        "home/skills/shared/SKILL.md",
        skill("shared", "user", "", "\nuser\n"),
    );

    let catalog = discover(&fixture);
    assert_eq!(catalog.entries().len(), 1);
    assert_eq!(catalog.entries()[0].origin, SkillOrigin::ProjectPlexmaton);
    assert_eq!(catalog.diagnostics().len(), 2);
    assert!(catalog.diagnostics().iter().all(|diagnostic| matches!(
        diagnostic.kind,
        SkillDiagnosticKind::Shadowed {
            winner: SkillOrigin::ProjectPlexmaton
        }
    )));
    let loaded = catalog
        .read(
            "shared",
            None,
            SkillInvocation::Model,
            &FileCancellation::new(),
        )
        .unwrap_or_else(|error| panic!("read winner: {error}"));
    assert_eq!(loaded.text, "\nnative\r\n");
    assert_eq!(loaded.digest.len(), 64);
    assert!(
        loaded
            .location
            .ends_with("/.plexmaton/skills/shared/SKILL.md")
    );

    let native_root = fixture.path().join("project/.plexmaton/skills");
    let original_root = fixture.path().join("project/.plexmaton/original-skills");
    fs::rename(&native_root, &original_root)
        .unwrap_or_else(|error| panic!("rename winning root: {error}"));
    fixture.write(
        "project/.plexmaton/skills/shared/SKILL.md",
        skill("shared", "replacement", "", "replacement\n"),
    );
    let still_pinned = catalog
        .read(
            "shared",
            None,
            SkillInvocation::Model,
            &FileCancellation::new(),
        )
        .unwrap_or_else(|error| panic!("read descriptor-pinned winner: {error}"));
    assert_eq!(still_pinned.text, "\nnative\r\n");

    fs::remove_file(original_root.join("shared/SKILL.md"))
        .unwrap_or_else(|error| panic!("remove winner: {error}"));
    assert!(matches!(
        catalog.read(
            "shared",
            None,
            SkillInvocation::Model,
            &FileCancellation::new()
        ),
        Err(SkillError::Read {
            source: BoundedReadError::Path(PathError::NotFound),
            ..
        })
    ));
}

/// SKL-2/SKL-3: configured child roots never follow symlinks outside their authority bases.
#[test]
fn derived_skill_root_symlinks_never_load_external_metadata() {
    let fixture = TestDir::new("root-symlinks");
    fixture.write(
        "external/skills/escape/SKILL.md",
        skill("escape", "outside", "", "outside"),
    );
    fs::create_dir_all(fixture.path().join("project"))
        .unwrap_or_else(|error| panic!("create project root: {error}"));
    fs::create_dir_all(fixture.path().join("home"))
        .unwrap_or_else(|error| panic!("create user root: {error}"));

    std::os::unix::fs::symlink(
        fixture.path().join("external"),
        fixture.path().join("project/.agents"),
    )
    .unwrap_or_else(|error| panic!("symlink .agents: {error}"));
    let catalog = discover(&fixture);
    assert!(catalog.entries().is_empty());
    assert!(catalog.diagnostics().iter().any(|diagnostic| {
        diagnostic.origin == SkillOrigin::ProjectAgents
            && diagnostic.kind == SkillDiagnosticKind::RootUnavailable
    }));

    fs::remove_file(fixture.path().join("project/.agents"))
        .unwrap_or_else(|error| panic!("remove .agents symlink: {error}"));
    fs::create_dir(fixture.path().join("project/.agents"))
        .unwrap_or_else(|error| panic!("create .agents: {error}"));
    std::os::unix::fs::symlink(
        fixture.path().join("external/skills"),
        fixture.path().join("project/.agents/skills"),
    )
    .unwrap_or_else(|error| panic!("symlink project skills: {error}"));
    let catalog = discover(&fixture);
    assert!(catalog.entries().is_empty());
    assert!(catalog.diagnostics().iter().any(|diagnostic| {
        diagnostic.origin == SkillOrigin::ProjectAgents
            && diagnostic.kind == SkillDiagnosticKind::RootUnavailable
    }));

    std::os::unix::fs::symlink(
        fixture.path().join("external/skills"),
        fixture.path().join("home/skills"),
    )
    .unwrap_or_else(|error| panic!("symlink user skills: {error}"));
    let catalog = discover(&fixture);
    assert!(catalog.entries().is_empty());
    assert!(catalog.diagnostics().iter().any(|diagnostic| {
        diagnostic.origin == SkillOrigin::User
            && diagnostic.kind == SkillDiagnosticKind::RootUnavailable
    }));
}

/// SKL-3/SKL-4: resources remain inside the winning bundle and retain its current policy.
#[test]
fn resources_are_exact_confined_and_recheck_invocation_after_replacement() {
    let fixture = TestDir::new("resources");
    fixture.write(
        "project/.agents/skills/deploy/SKILL.md",
        skill("deploy", "deploy", "", "instructions\n"),
    );
    fixture.write(
        "project/.agents/skills/deploy/references/note.md",
        b"exact\r\nresource",
    );
    fixture.write(
        "project/.agents/skills/model-only/SKILL.md",
        skill(
            "model-only",
            "model only",
            "user-invocable: false\n",
            "model instructions",
        ),
    );
    let catalog = discover(&fixture);

    assert!(
        catalog
            .read(
                "model-only",
                None,
                SkillInvocation::Model,
                &FileCancellation::new(),
            )
            .is_ok()
    );
    assert!(matches!(
        catalog.read(
            "model-only",
            None,
            SkillInvocation::User,
            &FileCancellation::new(),
        ),
        Err(SkillError::InvocationDenied { .. })
    ));

    let resource = catalog
        .read(
            "deploy",
            Some("references/note.md"),
            SkillInvocation::Model,
            &FileCancellation::new(),
        )
        .unwrap_or_else(|error| panic!("read resource: {error}"));
    assert_eq!(resource.resource.as_deref(), Some("references/note.md"));
    assert_eq!(resource.text, "exact\r\nresource");
    assert_eq!(resource.origin, SkillOrigin::ProjectAgents);
    assert!(matches!(
        catalog.read(
            "deploy",
            Some("../shared"),
            SkillInvocation::Model,
            &FileCancellation::new()
        ),
        Err(SkillError::InvalidResource)
    ));

    fixture.write(
        "project/.agents/skills/deploy/SKILL.md",
        skill(
            "deploy",
            "deploy",
            "disable-model-invocation: true\n",
            "new instructions\n",
        ),
    );
    assert!(matches!(
        catalog.read(
            "deploy",
            Some("references/note.md"),
            SkillInvocation::Model,
            &FileCancellation::new()
        ),
        Err(SkillError::InvocationDenied { .. })
    ));
    let user = catalog
        .read(
            "deploy",
            None,
            SkillInvocation::User,
            &FileCancellation::new(),
        )
        .unwrap_or_else(|error| panic!("user activation after replacement: {error}"));
    assert_eq!(user.text, "new instructions\n");

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            "/etc/passwd",
            fixture
                .path()
                .join("project/.agents/skills/deploy/references/escape"),
        )
        .unwrap_or_else(|error| panic!("create resource symlink: {error}"));
        assert!(matches!(
            catalog.read(
                "deploy",
                Some("references/escape"),
                SkillInvocation::User,
                &FileCancellation::new()
            ),
            Err(SkillError::Read {
                source: BoundedReadError::Path(PathError::Symlink),
                ..
            })
        ));
    }
}

/// SKL-2/SKL-4: malformed behavioral metadata is diagnostic and Unicode identity is normalized.
#[test]
fn metadata_is_strict_typed_and_unicode_normalized() {
    let fixture = TestDir::new("metadata");
    fixture.write(
        "home/skills/duplicate/SKILL.md",
        b"---\nname: duplicate\nname: duplicate\ndescription: duplicate\n---\nbody",
    );
    fixture.write(
        "home/skills/wrong-bool/SKILL.md",
        skill("wrong-bool", "wrong bool", "user-invocable: yes\n", "body"),
    );
    fixture.write(
        "home/skills/directory/SKILL.md",
        skill("different", "mismatch", "", "body"),
    );
    fixture.write(
        "home/skills/café/SKILL.md",
        skill("cafe\u{301}", "unicode", "unknown-key: ignored\n", "body"),
    );
    fixture.write(
        "home/skills/ignored-tree/SKILL.md",
        b"---\nname: ignored-tree\ndescription: bounded skip\nunknown:\n  template: &template\n    nested: [one, two, three, four]\n  aliases: [*template, *template, *template, *template, *template]\n---\nbody",
    );
    fixture.write(
        "home/skills/duplicate-unknown/SKILL.md",
        b"---\nname: duplicate-unknown\ndescription: duplicate unknown\nunknown: one\nunknown: two\n---\nbody",
    );

    let catalog = discover(&fixture);
    assert_eq!(catalog.entries().len(), 2);
    assert!(
        catalog
            .entries()
            .iter()
            .any(|entry| entry.name.as_str() == "café")
    );
    assert!(
        catalog
            .entries()
            .iter()
            .any(|entry| entry.name.as_str() == "ignored-tree")
    );
    let errors = catalog
        .diagnostics()
        .iter()
        .filter_map(|diagnostic| match &diagnostic.kind {
            SkillDiagnosticKind::InvalidMetadata { error } => Some(error),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 4);
    assert!(errors.contains(&&SkillMetadataError::InvalidYaml));
    assert!(errors.contains(&&SkillMetadataError::InvalidFieldType {
        field: SkillMetadataField::UserInvocable,
    }));
    assert!(errors.contains(&&SkillMetadataError::NameDirectoryMismatch));
}

/// SKL-2/SKL-3: discovery and content acquisition have independent hard bounds.
#[test]
fn candidate_frontmatter_catalog_and_content_bounds_are_typed() {
    let candidates = TestDir::new("candidate-limit");
    for index in 0..=MAX_SKILL_CANDIDATES {
        fs::create_dir_all(
            candidates
                .path()
                .join(format!("project/.agents/skills/candidate-{index}")),
        )
        .unwrap_or_else(|error| panic!("create candidate: {error}"));
    }
    assert!(matches!(
        SkillCatalog::discover(
            &candidates.path().join("home"),
            &candidates.path().join("project"),
            &FileCancellation::new(),
        ),
        Err(SkillError::CandidateLimit {
            limit: MAX_SKILL_CANDIDATES
        })
    ));

    let frontmatter = TestDir::new("frontmatter-limit");
    let oversized = format!(
        "---\nname: oversized\ndescription: oversized\nunknown: {}\n---\nbody",
        "x".repeat(MAX_FRONTMATTER_BYTES)
    );
    frontmatter.write("home/skills/oversized/SKILL.md", oversized);
    let catalog = discover(&frontmatter);
    assert!(matches!(
        catalog.diagnostics()[0].kind,
        SkillDiagnosticKind::InvalidMetadata {
            error: SkillMetadataError::FrontmatterTooLarge
        }
    ));

    let aggregate = TestDir::new("catalog-limit");
    // Forty raw 1 KiB descriptions fit beneath 64 KiB; their JSON escapes do not.
    for index in 0..40 {
        let name = format!("skill-{index}");
        aggregate.write(
            format!("home/skills/{name}/SKILL.md"),
            skill(&name, &format!("'{}'", "\\".repeat(1022)), "", "body"),
        );
    }
    assert!(matches!(
        SkillCatalog::discover(
            &aggregate.path().join("home"),
            &aggregate.path().join("project"),
            &FileCancellation::new(),
        ),
        Err(SkillError::CatalogLimit {
            limit: MAX_SKILL_CATALOG_BYTES
        })
    ));

    let content = TestDir::new("content-limit");
    content.write(
        "home/skills/large/SKILL.md",
        skill(
            "large",
            "large",
            "",
            &"x".repeat(MAX_SKILL_CONTENT_BYTES + 1),
        ),
    );
    let catalog = discover(&content);
    assert_eq!(
        catalog.read(
            "large",
            None,
            SkillInvocation::Model,
            &FileCancellation::new()
        ),
        Err(SkillError::ContentTooLarge {
            limit: MAX_SKILL_CONTENT_BYTES
        })
    );
}

/// SKL-2/SKL-3: cancellation is observed before discovery and before any loaded bytes return.
#[test]
fn cancellation_precedes_discovery_and_reads() {
    let fixture = TestDir::new("cancel");
    fixture.write(
        "home/skills/readable/SKILL.md",
        skill("readable", "readable", "", "body"),
    );
    let cancellation = FileCancellation::new();
    cancellation.cancel();
    assert!(matches!(
        SkillCatalog::discover(
            &fixture.path().join("home"),
            &fixture.path().join("project"),
            &cancellation,
        ),
        Err(SkillError::Cancelled)
    ));

    let catalog = discover(&fixture);
    assert_eq!(
        catalog.read("readable", None, SkillInvocation::User, &cancellation),
        Err(SkillError::Cancelled)
    );
}
