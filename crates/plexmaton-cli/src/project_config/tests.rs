use super::*;
use std::os::unix::fs::symlink;

const REGISTRY: &str = r#"
active_model = { provider = "user", model = "default" }

[providers.user]
base_url = "https://user.example/v1/"
api_key_env = "USER_API_KEY"
api = "openai_responses"

[providers.user.models.default]
id = "default-wire"
context_window_tokens = 10000
max_output_tokens = 1000
output_reserve_tokens = 500

[providers.project]
base_url = "https://project.example/v1/"
api_key_env = "PROJECT_API_KEY"
api = "anthropic_messages"

[providers.project.models.chosen]
id = "chosen-wire"
context_window_tokens = 20000
max_output_tokens = 2000
output_reserve_tokens = 1000
"#;

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("plexmaton-project-config-{}", uuid::Uuid::now_v7()));
        fs::create_dir(&path).expect("unique temporary directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn directory(&self, relative: &str) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(&path).expect("fixture directory");
        path
    }

    fn write(&self, relative: &str, contents: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent");
        }
        fs::write(&path, contents).expect("fixture file");
        path
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _cleanup = fs::remove_dir_all(&self.0);
    }
}

fn select_model(
    root: &Path,
    registry: &ModelRegistry,
) -> Result<ResolvedModel, ProjectConfigError> {
    load(root)?.select_model(registry)
}

fn registry() -> ModelRegistry {
    ModelRegistry::parse(REGISTRY).expect("valid registry fixture")
}

#[test]
fn discovers_nearest_checkout_from_a_nested_directory() {
    // SKL-1: project authority is the nearest physical checkout containing a valid Git marker.
    let fixture = TempDirectory::new();
    fixture.directory("repo/.git");
    let nested = fixture.directory("repo/src/deep");

    assert_eq!(
        discover_project_root(&nested).expect("project root"),
        fixture.path().join("repo").canonicalize().expect("repo")
    );
    assert_eq!(
        discover_project_root(&fixture.path().join("repo")).expect("root itself"),
        fixture.path().join("repo").canonicalize().expect("repo")
    );
}

#[test]
fn non_git_directory_remains_its_own_project_root() {
    let fixture = TempDirectory::new();
    let starting = fixture.directory("plain/nested");

    assert_eq!(
        discover_project_root(&starting).expect("fallback root"),
        starting.canonicalize().expect("starting directory")
    );
}

#[test]
fn linked_worktree_uses_its_checkout_instead_of_the_common_repo() {
    let fixture = TempDirectory::new();
    let gitdir = fixture.directory("common.git/worktrees/linked");
    let checkout = fixture.directory("linked/src");
    fixture.write(
        "common.git/worktrees/linked/.plexmaton/config.toml",
        "credentials = \"must-not-load\"\n",
    );
    fixture.write("linked/.git", format!("gitdir: {}\n", gitdir.display()));

    let root = discover_project_root(&checkout).expect("worktree root");
    assert_eq!(
        root,
        fixture
            .path()
            .join("linked")
            .canonicalize()
            .expect("checkout")
    );
    let selected = select_model(&root, &registry()).expect("checkout-local configuration");
    assert_eq!(selected.provider_name(), "user");
    assert_eq!(selected.model_name(), "default");
}

#[test]
fn project_selection_overrides_only_the_user_active_model() {
    let fixture = TempDirectory::new();
    fixture.write(
        ".plexmaton/config.toml",
        "[active_model]\nprovider = \"project\"\nmodel = \"chosen\"\n",
    );

    let selected = select_model(fixture.path(), &registry()).expect("project selection");
    assert_eq!(selected.provider_name(), "project");
    assert_eq!(selected.model_name(), "chosen");
}

#[test]
fn missing_and_empty_project_config_preserve_the_user_selection() {
    let missing = TempDirectory::new();
    let empty = TempDirectory::new();
    empty.write(".plexmaton/config.toml", "");

    for root in [missing.path(), empty.path()] {
        let selected = select_model(root, &registry()).expect("user selection");
        assert_eq!(selected.provider_name(), "user");
        assert_eq!(selected.model_name(), "default");
    }
}

#[test]
fn project_config_rejects_authority_and_unrecognized_fields() {
    let fixture = TempDirectory::new();
    for source in [
        "[providers.local]\nbase_url = \"https://example.test\"\n",
        "credentials = \"secret\"\n",
        "session_root = \"sessions\"\n",
        "statusline = \"echo unsafe\"\n",
        "[active_model]\nprovider = \"project\"\nmodel = \"chosen\"\neffort = \"high\"\n",
    ] {
        fixture.write(".plexmaton/config.toml", source);
        assert!(matches!(
            select_model(fixture.path(), &registry()),
            Err(ProjectConfigError::InvalidConfiguration)
        ));
    }
}

#[test]
fn malformed_oversized_and_unknown_selections_are_diagnostics() {
    let fixture = TempDirectory::new();
    fixture.write(".plexmaton/config.toml", "[active_model\n");
    assert!(matches!(
        select_model(fixture.path(), &registry()),
        Err(ProjectConfigError::InvalidConfiguration)
    ));

    fixture.write(".plexmaton/config.toml", [0xff]);
    assert!(matches!(
        select_model(fixture.path(), &registry()),
        Err(ProjectConfigError::InvalidUtf8)
    ));

    fixture.write(
        ".plexmaton/config.toml",
        vec![b' '; MAX_PROJECT_CONFIG_BYTES],
    );
    let selected = select_model(fixture.path(), &registry()).expect("exact byte bound");
    assert_eq!(selected.model_name(), "default");

    fixture.write(
        ".plexmaton/config.toml",
        vec![b' '; MAX_PROJECT_CONFIG_BYTES + 1],
    );
    assert!(matches!(
        select_model(fixture.path(), &registry()),
        Err(ProjectConfigError::TooLarge)
    ));

    fixture.write(
        ".plexmaton/config.toml",
        "active_model = { provider = \"project\", model = \"absent\" }\n",
    );
    assert!(matches!(
        select_model(fixture.path(), &registry()),
        Err(ProjectConfigError::UnknownModel { provider, model })
            if provider == "project" && model == "absent"
    ));
}

#[test]
fn present_non_file_config_is_a_read_diagnostic() {
    let fixture = TempDirectory::new();
    fixture.directory(".plexmaton/config.toml");

    assert!(matches!(
        select_model(fixture.path(), &registry()),
        Err(ProjectConfigError::Read(BoundedReadError::Path(
            PathError::NotFile
        )))
    ));
}

#[test]
fn project_config_refuses_symlinks_at_every_component() {
    let target = TempDirectory::new();
    target.write("config.toml", "");

    let linked_file = TempDirectory::new();
    linked_file.directory(".plexmaton");
    symlink(
        target.path().join("config.toml"),
        linked_file.path().join(".plexmaton/config.toml"),
    )
    .expect("config symlink");
    assert!(matches!(
        select_model(linked_file.path(), &registry()),
        Err(ProjectConfigError::Read(BoundedReadError::Path(
            PathError::Symlink
        )))
    ));

    let linked_directory = TempDirectory::new();
    symlink(target.path(), linked_directory.path().join(".plexmaton")).expect("directory symlink");
    assert!(matches!(
        select_model(linked_directory.path(), &registry()),
        Err(ProjectConfigError::Read(BoundedReadError::Path(
            PathError::Symlink
        )))
    ));
}

#[test]
fn invalid_gitfile_does_not_claim_an_ancestor() {
    let fixture = TempDirectory::new();
    let starting = fixture.directory("invalid/nested");
    fixture.write("invalid/.git", "gitdir: missing\n");

    assert_eq!(
        discover_project_root(&starting).expect("fallback root"),
        starting.canonicalize().expect("starting directory")
    );
}

mod permissions;
