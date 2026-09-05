//! Startup composition resolves project settings and skills before acquiring the terminal.

use std::{ffi::OsString, fs, path::PathBuf};

use anyhow::Context as _;
use plexmaton_core::AgentId;
use plexmaton_provider::{resolve_api_key, resolve_home};
use plexmaton_runtime::NativeToolCatalog;

use crate::{
    INTERNAL_RG_DRIVER, project_config, resolve_path_executable,
    session::{OpenedSession, SessionSelection, open_selected_session},
    session_picker, statusline,
};

pub(super) async fn live_runtime_from_process(
    selection: SessionSelection,
) -> anyhow::Result<(
    OpenedSession,
    PathBuf,
    session_picker::SessionPicker,
    Option<statusline::StatusLine>,
)> {
    let configured_home = std::env::var_os("PLEXMATON_HOME");
    let user_home = std::env::var_os("HOME").map(PathBuf::from);
    let root = resolve_home(configured_home.as_deref(), user_home.as_deref())
        .context("resolve Plexmaton configuration root")?;
    let path = root.join("config.toml");
    let source = fs::read_to_string(&path)
        .with_context(|| format!("read provider configuration at {}", path.display()))?;
    let (config, status_config) = statusline::parse(&source)?;
    let workspace_root = std::env::current_dir()
        .context("resolve tool workspace")?
        .canonicalize()
        .context("canonicalize tool workspace")?;
    let project_root = project_config::discover_project_root(&workspace_root)
        .context("resolve project configuration root")?;
    let model = project_config::select_model(&project_root, &config)
        .context("resolve project model selection")?;
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
    .with_skill_roots(
        &root,
        &project_root,
        &plexmaton_file_tools::FileCancellation::new(),
    )
    .context("discover project and user skills")?;
    let picker = session_picker::SessionPicker::new(session_picker::Launcher {
        root: root.clone(),
        workspace: workspace_root.clone(),
        model: model.clone(),
        ripgrep,
        driver,
    });
    let agent_id = AgentId::new("agent-primary").context("build primary agent identity")?;
    let status_line = status_config
        .map(|config| statusline::StatusLine::new(config, model.clone(), workspace_root.clone()));
    let opened = open_selected_session(&root, selection, agent_id, model, key, tools).await?;
    Ok((opened, workspace_root, picker, status_line))
}
