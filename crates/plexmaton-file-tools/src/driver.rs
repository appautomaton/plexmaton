//! Minimal descriptor-rooted handoff from the composition executable to ripgrep.

use std::{
    ffi::OsString,
    io,
    os::unix::process::CommandExt as _,
    process::{Command, Stdio},
};

/// Changes to the directory supplied on stdin, then replaces this process with ripgrep.
///
/// The caller must map a pinned directory descriptor to stdin and pass the ripgrep executable as
/// the first argument. `exec` preserves the child identity, pipes, and cancellation ownership.
pub fn run_search_driver(arguments: impl IntoIterator<Item = OsString>) -> io::Result<()> {
    let mut arguments = arguments.into_iter();
    let executable = arguments
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing ripgrep executable"))?;
    rustix::process::fchdir(io::stdin()).map_err(io::Error::from)?;
    let error = Command::new(executable)
        .args(arguments)
        .stdin(Stdio::null())
        .exec();
    Err(error)
}
