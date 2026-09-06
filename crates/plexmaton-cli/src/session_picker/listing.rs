//! Non-mutating bounded previews. Full validation and exclusive ownership happen only on open.
use super::*;
use plexmaton_tui::MAX_CONVERSATION_CHOICES;
use std::io::{BufRead as _, Read as _};
use std::time::SystemTime;

const MAX_DIRECTORY_ENTRIES: usize = 10_000;
const PREVIEW_BYTES: u64 = 64 * 1024;

pub(super) fn list(
    root: &Path,
    cancel: &CancellationToken,
) -> anyhow::Result<(Vec<ConversationChoice>, bool)> {
    let directory = root.join("sessions");
    match fs::symlink_metadata(&directory) {
        Ok(metadata) => anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "invalid sessions directory"
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((Vec::new(), false)),
        Err(error) => return Err(error.into()),
    }
    let read = match fs::read_dir(&directory) {
        Ok(read) => read,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((Vec::new(), false)),
        Err(error) => return Err(error.into()),
    };
    let mut candidates = Vec::new();
    let mut limited = false;
    for (index, entry) in read.enumerate() {
        if cancel.is_cancelled() {
            break;
        }
        if index >= MAX_DIRECTORY_ENTRIES {
            limited = true;
            break;
        }
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension() != Some(OsStr::new("jsonl")) {
            continue;
        }
        let Some(name) = path.file_stem().and_then(OsStr::to_str) else {
            continue;
        };
        if name.len() > 128
            || !name
                .bytes()
                .enumerate()
                .all(|(i, b)| b.is_ascii_alphanumeric() || (i > 0 && matches!(b, b'-' | b'_')))
        {
            continue;
        }
        let Ok(id) = ConversationId::new(name) else {
            continue;
        };
        let modified = entry
            .metadata()?
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH);
        candidates.push((modified, id, path));
        candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.as_str().cmp(b.1.as_str())));
        if candidates.len() > MAX_CONVERSATION_CHOICES {
            candidates.pop();
            limited = true;
        }
    }
    let mut entries = Vec::new();
    for (_, id, path) in candidates {
        if cancel.is_cancelled() {
            break;
        }
        let preview = preview(&path).unwrap_or_default();
        let title = if preview.is_empty() {
            id.as_str().to_owned()
        } else {
            format!("{preview} · {}", id.as_str())
        };
        entries.push(ConversationChoice { id, title });
    }
    Ok((entries, limited))
}

fn preview(path: &Path) -> io::Result<String> {
    // O_NOFOLLOW protects previews if an entry becomes a symlink after enumeration.
    use std::os::unix::fs::OpenOptionsExt as _;
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Ok(String::new());
    }
    let mut reader = io::BufReader::new(file.take(PREVIEW_BYTES));
    let mut line = String::new();
    while reader.read_line(&mut line)? != 0 {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
            let payload = &value["entry"]["payload"];
            if payload["type"] == "turn_started" {
                return Ok(payload["text"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(80)
                    .collect());
            }
        }
        line.clear();
    }
    Ok(String::new())
}
