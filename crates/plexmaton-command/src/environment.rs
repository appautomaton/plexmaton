//! Frozen child-process environment construction (CMD-2).

use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    path::Path,
};

use tokio::process::Command;

const FALLBACK_PATH: &str = "/usr/bin:/bin";
const FALLBACK_LOCALE: &str = "C.UTF-8";

/// Session-owned environment inherited by foreground commands.
///
/// Capture happens once when the command tool is constructed. Provider credentials and private
/// Plexmaton authority are removed at that boundary; non-Unicode variables remain byte-exact.
#[derive(Clone)]
pub(crate) struct CommandEnvironment {
    inherited: BTreeMap<OsString, OsString>,
}

impl CommandEnvironment {
    pub(crate) fn capture() -> Self {
        Self::from_iter(std::env::vars_os())
    }

    fn from_iter(entries: impl IntoIterator<Item = (OsString, OsString)>) -> Self {
        let inherited = entries
            .into_iter()
            .filter(|(key, _)| !is_private_environment_key(key))
            .collect();
        Self { inherited }
    }

    pub(crate) fn install(&self, command: &mut Command, workspace_root: &Path) {
        command.env_clear().envs(&self.inherited);
        if !self.inherited.contains_key(OsStr::new("PATH")) {
            command.env("PATH", FALLBACK_PATH);
        }
        if !self.inherited.contains_key(OsStr::new("LANG"))
            && !self.inherited.contains_key(OsStr::new("LC_ALL"))
            && !self.inherited.contains_key(OsStr::new("LC_CTYPE"))
        {
            command.env("LANG", FALLBACK_LOCALE);
        }
        command
            .env("PWD", workspace_root)
            .env_remove("OLDPWD")
            .env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .env("PAGER", "cat")
            .env("GIT_PAGER", "cat")
            .env("GH_PAGER", "cat")
            .env("GIT_TERMINAL_PROMPT", "0");
    }

    #[cfg(test)]
    pub(crate) fn from_pairs(entries: impl IntoIterator<Item = (OsString, OsString)>) -> Self {
        Self::from_iter(entries)
    }
}

fn is_private_environment_key(key: &OsStr) -> bool {
    let Some(key) = key.to_str() else {
        return false;
    };
    let key = key.to_ascii_uppercase();
    key.starts_with("PLEXMATON_")
        || key.ends_with("_API_KEY")
        || key.ends_with("_TOKEN")
        || key.ends_with("_SECRET")
        || key.ends_with("_PASSWORD")
        || matches!(
            key.as_str(),
            "API_KEY"
                | "TOKEN"
                | "SECRET"
                | "PASSWORD"
                | "AWS_ACCESS_KEY_ID"
                | "AWS_SECRET_ACCESS_KEY"
        )
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::CommandEnvironment;

    #[test]
    fn cmd_2_environment_snapshot_preserves_non_unicode_entries() {
        use std::os::unix::ffi::OsStringExt;

        let key = OsString::from_vec(vec![b'T', b'O', b'O', b'L', 0xff]);
        let value = OsString::from_vec(vec![b'v', 0xfe]);
        let environment = CommandEnvironment::from_pairs([(key.clone(), value.clone())]);
        assert_eq!(environment.inherited.get(&key), Some(&value));
    }

    #[test]
    fn cmd_2_environment_snapshot_scrubs_credential_shaped_names() {
        let environment = CommandEnvironment::from_pairs(
            [
                "PLEXMATON_HOME",
                "OPENAI_API_KEY",
                "GITHUB_TOKEN",
                "DATABASE_PASSWORD",
                "DEPLOY_SECRET",
                "AWS_ACCESS_KEY_ID",
                "AWS_SECRET_ACCESS_KEY",
            ]
            .map(|key| (OsString::from(key), OsString::from("must-not-leak"))),
        );
        assert!(environment.inherited.is_empty());
    }
}
