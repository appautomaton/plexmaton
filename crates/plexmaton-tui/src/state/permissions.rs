//! Presentation-only permission controls. All mutations echo a producer revision and await its reply.
use plexmaton_core::{
    NativeFilePreset, PermissionAction, PermissionChangeError, PermissionIntent, PermissionScope,
    PermissionStateView,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PermissionChoice {
    Configuration,
    Review(PermissionAction),
    Confirm(PermissionIntent),
    Back,
    Reload,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PermissionPage {
    Configuration {
        view: PermissionStateView,
    },
    Loading,
    Browse {
        view: PermissionStateView,
        changed: Option<Result<(), PermissionChangeError>>,
    },
    Confirm {
        view: PermissionStateView,
        intent: PermissionIntent,
    },
    Submitting,
    Unavailable(PermissionChangeError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PermissionPanel {
    page: PermissionPage,
    selected: usize,
}

impl PermissionPanel {
    pub(crate) fn preferred_rows(&self, width: u16) -> u16 {
        if self.is_reading() {
            return 22;
        }
        let width = crate::surface::ContentInsets::for_surface(crate::SurfaceId::Drawer, u16::MAX)
            .width(width);
        let heading = self
            .description()
            .iter()
            .flat_map(|line| super::wrap_line(line, usize::from(width)))
            .count()
            .min(8);
        u16::try_from(heading + self.choices().len().min(6) + 7)
            .unwrap_or(u16::MAX)
            .max(10)
    }
    pub(crate) const fn loading() -> Self {
        Self {
            page: PermissionPage::Loading,
            selected: 0,
        }
    }

    pub(crate) fn loaded(
        &mut self,
        view: Result<PermissionStateView, PermissionChangeError>,
        changed: Option<Result<(), PermissionChangeError>>,
    ) {
        self.page = match view {
            Ok(view) => PermissionPage::Browse { view, changed },
            Err(reason) => PermissionPage::Unavailable(reason),
        };
        self.selected = 0;
    }

    pub(crate) const fn is_reading(&self) -> bool {
        matches!(self.page, PermissionPage::Configuration { .. })
    }

    pub(crate) fn choices(&self) -> Vec<(PermissionChoice, String)> {
        match &self.page {
            PermissionPage::Configuration { view } => match &view.configuration {
                Some(config)
                    if !config.trusted
                        && config.rules.iter().any(|rule| {
                            rule.action == plexmaton_core::PermissionRuleAction::Allow
                        }) =>
                {
                    vec![(
                        PermissionChoice::Review(PermissionAction::TrustProjectConfiguration(
                            config.fingerprint,
                        )),
                        "Continue to activation…".to_owned(),
                    )]
                }
                _ => vec![(PermissionChoice::Back, "Back".to_owned())],
            },
            PermissionPage::Browse { view, .. } => {
                let mut choices = Vec::new();
                match &view.native_files {
                    NativeFilePreset::Disabled => choices.push((
                        PermissionChoice::Review(PermissionAction::EnableNativeFiles),
                        "Enable native file changes for this Session…".to_owned(),
                    )),
                    NativeFilePreset::Enabled(id) => choices.push((
                        PermissionChoice::Review(PermissionAction::Revoke(id.clone())),
                        "Turn off Session file changes…".to_owned(),
                    )),
                    NativeFilePreset::Unavailable => {}
                }
                for grant in &view.grants {
                    if matches!(&view.native_files, NativeFilePreset::Enabled(id) if id == &grant.id)
                    {
                        continue;
                    }
                    let scope = match grant.scope {
                        PermissionScope::Session => "Session",
                        PermissionScope::Project => "Project",
                    };
                    choices.push((
                        PermissionChoice::Review(PermissionAction::Revoke(grant.id.clone())),
                        format!("Revoke {scope}: {}", grant.label),
                    ));
                }
                if view
                    .configuration
                    .as_ref()
                    .is_some_and(|config| !config.rules.is_empty())
                {
                    choices.push((
                        PermissionChoice::Configuration,
                        "Review project configuration rules…".to_owned(),
                    ));
                }
                if view.trusted_config.is_some() {
                    choices.push((
                        PermissionChoice::Review(PermissionAction::RevokeProjectTrust),
                        "Withdraw project configuration trust…".to_owned(),
                    ));
                }
                choices.push((PermissionChoice::Reload, "Refresh permissions".to_owned()));
                choices
            }
            PermissionPage::Confirm { intent, .. } => vec![
                (
                    PermissionChoice::Confirm(intent.clone()),
                    match intent.action {
                        PermissionAction::EnableNativeFiles => "Enable for this Session",
                        PermissionAction::Revoke(_) => "Revoke permission",
                        PermissionAction::TrustProjectConfiguration(_) => {
                            "Activate these Allow rules"
                        }
                        PermissionAction::RevokeProjectTrust => "Withdraw trust",
                    }
                    .to_owned(),
                ),
                (PermissionChoice::Back, "Back".to_owned()),
            ],
            PermissionPage::Unavailable(_) => {
                vec![(PermissionChoice::Reload, "Reload permissions".to_owned())]
            }
            PermissionPage::Loading | PermissionPage::Submitting => Vec::new(),
        }
    }

    pub(crate) const fn selected(&self) -> usize {
        self.selected
    }

    pub(crate) fn hint(&self) -> &'static str {
        if self.is_reading() {
            if self
                .choices()
                .first()
                .is_some_and(|(choice, _)| *choice == PermissionChoice::Back)
            {
                "↑↓ read · Enter/Esc back"
            } else {
                "↑↓ read · Enter continue · Esc back"
            }
        } else if matches!(self.page, PermissionPage::Confirm { .. }) {
            "↑↓ choose · Enter confirm · Esc back"
        } else {
            "↑↓ choose · Enter select · Esc close"
        }
    }

    pub(crate) fn step(&mut self, forward: bool) -> bool {
        let last = self.choices().len().saturating_sub(1);
        let next = if forward {
            self.selected.saturating_add(1).min(last)
        } else {
            self.selected.saturating_sub(1)
        };
        let changed = next != self.selected;
        self.selected = next;
        changed
    }

    pub(crate) fn activate(&mut self, choice: &PermissionChoice) -> Option<PermissionIntent> {
        if !self.choices().iter().any(|(current, _)| current == choice) {
            return None;
        }
        match choice {
            PermissionChoice::Configuration => {
                let PermissionPage::Browse { view, .. } = &self.page else {
                    return None;
                };
                self.page = PermissionPage::Configuration { view: view.clone() };
                self.selected = 0;
            }
            PermissionChoice::Review(action) => {
                let (PermissionPage::Browse { view, .. } | PermissionPage::Configuration { view }) =
                    &self.page
                else {
                    return None;
                };
                self.page = PermissionPage::Confirm {
                    intent: PermissionIntent {
                        expected: view.revision.clone(),
                        action: action.clone(),
                    },
                    view: view.clone(),
                };
                self.selected = 1;
            }
            PermissionChoice::Confirm(intent) => {
                self.page = PermissionPage::Submitting;
                return Some(intent.clone());
            }
            PermissionChoice::Back => {
                self.back();
            }
            PermissionChoice::Reload => {
                self.page = PermissionPage::Loading;
            }
        }
        None
    }

    pub(crate) fn back(&mut self) -> bool {
        self.page = match &self.page {
            PermissionPage::Confirm { view, intent }
                if matches!(
                    intent.action,
                    PermissionAction::TrustProjectConfiguration(_)
                ) =>
            {
                PermissionPage::Configuration { view: view.clone() }
            }
            PermissionPage::Confirm { view, .. } | PermissionPage::Configuration { view } => {
                PermissionPage::Browse {
                    view: view.clone(),
                    changed: None,
                }
            }
            _ => return false,
        };
        self.selected = 0;
        true
    }

    pub(crate) fn description(&self) -> Vec<String> {
        match &self.page {
            PermissionPage::Configuration { view } => configuration_description(view),
            PermissionPage::Loading => vec!["Loading permissions…".to_owned()],
            PermissionPage::Submitting => vec!["Applying change… Waiting for confirmation.".to_owned()],
            PermissionPage::Unavailable(reason) => vec![reason.to_string()],
            PermissionPage::Browse { view, changed } => {
                let mut text = vec!["Session grants last until Plexmaton exits.".to_owned(), "They stay active across /new and resume.".to_owned()];
                match view.project {
                    plexmaton_core::ProjectPermissionSource::Available => text.push("Project grants survive Sessions and restarts.".to_owned()),
                    plexmaton_core::ProjectPermissionSource::Unavailable => text.push("Project permissions unavailable; tools cannot run.".to_owned()),
                    plexmaton_core::ProjectPermissionSource::Disabled => {},
                }
                if let Some(config) = &view.configuration {
                    text.push(if config.trusted { "Project configuration Allow rules are active." } else { "Project Allow rules need explicit trust. Ask and Deny are active." }.to_owned());
                }
                if let Some(changed) = changed {
                    text.push(match changed {
                        Ok(()) => "Permission updated. Other rules may still allow matching operations.".to_owned(),
                        Err(reason) => reason.to_string(),
                    });
                }
                text
            }
            PermissionPage::Confirm { view, intent } => match &intent.action {
                PermissionAction::TrustProjectConfiguration(fingerprint) => vec![
                    "Activate the reviewed project Allow rules?".to_owned(),
                    format!("Configuration SHA-256: {}", fingerprint_text(fingerprint)),
                    "Personal trust survives restarts. Editing the file requires a new review; Ask and Deny still take precedence.".to_owned(),
                ],
                PermissionAction::RevokeProjectTrust => vec![
                    "Withdraw project configuration trust?".to_owned(),
                    "Configured Allow rules will stop granting permission. Ask, Deny and separately remembered grants remain in effect.".to_owned(),
                ],
                PermissionAction::EnableNativeFiles => vec![
                    "Allow native create/edit in this workspace?".to_owned(),
                    "Excludes agent controls, configuration paths and Git metadata. Commands need their own permission.".to_owned(),
                    "Until Plexmaton exits; kept across conversations.".to_owned(),
                ],
                PermissionAction::Revoke(id) => vec![
                    "Revoke this permission?".to_owned(),
                    view.grants.iter().find(|grant| &grant.id == id).map_or_else(String::new, |grant| grant.label.clone()),
                    "Other rules may still allow matching operations.".to_owned(),
                ],
            },
        }
    }
}

impl super::ViewState {
    pub(crate) fn activate_permission(
        &mut self,
        choice: &PermissionChoice,
    ) -> Option<PermissionIntent> {
        let panel = self.drawer.as_mut()?.permissions_mut()?;
        let intent = panel.activate(choice);
        self.scroll.reset_panel(crate::SurfaceId::Drawer);
        self.touch();
        intent
    }
    pub(crate) fn open_permissions(&mut self) {
        self.show_page(super::Shown::Permissions(Box::new(
            PermissionPanel::loading(),
        )));
    }

    pub(crate) fn update_permissions(
        &mut self,
        view: Result<PermissionStateView, PermissionChangeError>,
        changed: Option<Result<(), PermissionChangeError>>,
    ) {
        if let Some(panel) = self
            .drawer
            .as_mut()
            .and_then(super::Drawer::permissions_mut)
        {
            panel.loaded(view, changed);
            self.scroll.reset_panel(crate::SurfaceId::Drawer);
            self.touch();
        }
    }

    pub(crate) fn permission_back(&mut self) -> bool {
        let changed = self
            .drawer
            .as_mut()
            .and_then(super::Drawer::permissions_mut)
            .is_some_and(PermissionPanel::back);
        if changed {
            self.scroll.reset_panel(crate::SurfaceId::Drawer);
            self.touch();
        }
        changed
    }
}

fn fingerprint_text(fingerprint: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    fingerprint
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            let _written = write!(text, "{byte:02x}");
            text
        })
}

fn configuration_description(view: &PermissionStateView) -> Vec<String> {
    let Some(config) = &view.configuration else {
        return Vec::new();
    };
    let mut text = vec![
        "Project: .plexmaton/config.toml".to_owned(),
        if config.trusted {
            "Allow rules active for these exact bytes."
        } else {
            "Allow rules inactive until personally trusted."
        }
        .to_owned(),
        "Ask and Deny take precedence over Allow and remembered grants.".to_owned(),
        String::new(),
    ];
    for (index, rule) in config.rules.iter().enumerate() {
        let action = match rule.action {
            plexmaton_core::PermissionRuleAction::Allow => "Allow",
            plexmaton_core::PermissionRuleAction::Ask => "Ask",
            plexmaton_core::PermissionRuleAction::Deny => "Deny",
        };
        text.push(format!("{}. {action}: {}", index + 1, rule.label));
        text.push(String::new());
    }
    text.push(format!(
        "SHA-256: {}",
        fingerprint_text(&config.fingerprint)
    ));
    text
}
