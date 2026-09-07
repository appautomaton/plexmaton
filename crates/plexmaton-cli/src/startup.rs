//! Startup composition resolves project settings and skills before acquiring the terminal.

use std::{ffi::OsString, fs, path::PathBuf};

use anyhow::Context as _;
use plexmaton_core::AgentId;
use plexmaton_provider::{resolve_api_key, resolve_home};
use plexmaton_runtime::NativeToolCatalog;

use crate::{
    INTERNAL_RG_DRIVER, agent_instructions, project_config, resolve_path_executable,
    session::{ConversationSelection, OpenedConversation, open_selected_conversation},
    session_picker, statusline,
};

pub(super) async fn live_runtime_from_process(
    selection: ConversationSelection,
) -> anyhow::Result<(
    OpenedConversation,
    PathBuf,
    session_picker::ConversationPicker,
    Option<statusline::StatusLine>,
)> {
    let configured_home = std::env::var_os("PLEXMATON_HOME");
    let user_home = std::env::var_os("HOME").map(PathBuf::from);
    let root = resolve_home(configured_home.as_deref(), user_home.as_deref())
        .context("resolve Plexmaton configuration root")?;
    let path = root.join("config.toml");
    let source = fs::read_to_string(&path)
        .with_context(|| format!("read provider configuration at {}", path.display()))?;
    let config = crate::user_config::parse(&source)?;
    let workspace_root = std::env::current_dir()
        .context("resolve tool workspace")?
        .canonicalize()
        .context("canonicalize tool workspace")?;
    let project_root = project_config::discover_project_root(&workspace_root)
        .context("resolve project configuration root")?;
    let configured_model = project_config::load(&project_root)?
        .select_model(&config.models)
        .context("resolve project model selection")?;
    let model = agent_instructions::resolve_model(
        &configured_model,
        &root,
        &project_root,
        &workspace_root,
        &plexmaton_file_tools::FileCancellation::new(),
    )
    .context("load AGENTS.md instructions")?;
    let key = resolve_api_key(&model, std::env::var_os(model.api_key_env()))
        .context("resolve provider API key")?;
    let ripgrep = resolve_path_executable("rg", std::env::var_os("PATH").as_deref())?;
    let driver = std::env::current_exe()
        .context("resolve Plexmaton executable for the search driver")?
        .canonicalize()
        .context("canonicalize Plexmaton search driver")?;
    let tools = NativeToolCatalog::open(
        &workspace_root,
        model.api_key_env(),
        ripgrep.clone(),
        driver.clone(),
        vec![OsString::from(INTERNAL_RG_DRIVER)],
    )
    .context("configure native workspace tools")?
    .with_provider_credentials(config.models.models().map(|model| model.api_key_env()))
    .with_skill_roots(
        &root,
        &project_root,
        &plexmaton_file_tools::FileCancellation::new(),
    )
    .context("discover project and user skills")?;
    let compiler = tools.permission_compiler();
    let user_rules = config
        .permissions
        .compile(&compiler, |index| {
            plexmaton_agent::PermissionRuleSource::UserConfiguration {
                fingerprint: config.fingerprint,
                index,
            }
        })
        .context("compile user permission rules")?;
    let project_store =
        plexmaton_permission_store::ProjectPermissionStore::open(&root, &project_root)
            .context("open personal project permissions")?;
    let permissions = plexmaton_runtime::CodingSessionPermissions::new(&tools)
        .with_user_rules(user_rules)?
        .with_project_store(project_store)?
        .with_project_configuration(std::sync::Arc::new(
            project_config::ProjectPermissionReader::new(project_root, compiler),
        ))?;
    let picker = session_picker::ConversationPicker::new(session_picker::Launcher {
        root: root.clone(),
        workspace: workspace_root.clone(),
        model: configured_model,
        models: config.models.clone(),
        ripgrep,
        driver,
        permissions: permissions.clone(),
    });
    let agent_id = AgentId::new("agent-primary").context("build primary agent identity")?;
    let status_line = config.status_line.map(|status| {
        statusline::StatusLine::new(status, model.clone(), workspace_root.clone())
            .with_provider_credentials(
                config
                    .models
                    .models()
                    .map(|model| model.api_key_env().to_owned()),
            )
    });
    let mut opened =
        open_selected_conversation(&root, selection, agent_id, model, key, tools).await?;
    opened.runtime.use_coding_session(permissions)?;
    Ok((opened, workspace_root, picker, status_line))
}
