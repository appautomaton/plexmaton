//! The two configuration readers share one strict declaration grammar and catalog compiler.
use plexmaton_agent::{
    MAX_PERMISSION_ENTRIES, PermissionChangeError, PermissionRule, PermissionRuleAction,
    PermissionRuleSource,
};
use plexmaton_runtime::NativePermissionCompiler;
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(try_from = "Declarations")]
pub(crate) struct PermissionDeclarations(Vec<Rule>);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Declarations {
    #[serde(default)]
    rules: Vec<Rule>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    action: PermissionRuleAction,
    #[serde(rename = "match")]
    matcher: Matcher,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Matcher {
    NativeFileChanges {},
    ExactCommand { source: String },
    CommandPrefix { arguments: Vec<String> },
}

impl TryFrom<Declarations> for PermissionDeclarations {
    type Error = PermissionChangeError;

    fn try_from(value: Declarations) -> Result<Self, Self::Error> {
        if value.rules.len() > MAX_PERMISSION_ENTRIES {
            return Err(PermissionChangeError::Capacity);
        }
        Ok(Self(value.rules))
    }
}

impl PermissionDeclarations {
    pub(crate) fn compile(
        &self,
        compiler: &NativePermissionCompiler,
        source: impl Fn(u16) -> PermissionRuleSource,
    ) -> Result<Vec<PermissionRule>, PermissionChangeError> {
        self.0
            .iter()
            .enumerate()
            .map(|(index, rule)| {
                let matcher = match &rule.matcher {
                    Matcher::NativeFileChanges {} => compiler.native_file_changes(),
                    Matcher::CommandPrefix { arguments } => compiler
                        .command_prefix(arguments.clone())
                        .ok_or(PermissionChangeError::Unavailable)?,
                    Matcher::ExactCommand { source } => compiler
                        .exact_command(source)
                        .ok_or(PermissionChangeError::Unavailable)?,
                };
                Ok(PermissionRule {
                    source: source(
                        u16::try_from(index).map_err(|_| PermissionChangeError::Capacity)?,
                    ),
                    matcher,
                    action: rule.action,
                })
            })
            .collect()
    }
}
