use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    task::JoinHandle,
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);
const TIMEOUT: Duration = Duration::from_secs(5);

pub(super) struct Scratch(PathBuf);

impl Scratch {
    pub(super) fn new(label: &str) -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "plexmaton-runtime-skills-{label}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap_or_else(|error| panic!("create fixture root: {error}"));
        fs::create_dir_all(path.join("project"))
            .unwrap_or_else(|error| panic!("create fixture project: {error}"));
        fs::create_dir_all(path.join("home"))
            .unwrap_or_else(|error| panic!("create fixture home: {error}"));
        Self(path)
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }

    pub(super) fn project(&self) -> PathBuf {
        self.0.join("project")
    }

    pub(super) fn user_home(&self) -> PathBuf {
        self.0.join("home")
    }

    pub(super) fn write(&self, relative: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("create fixture parent: {error}"));
        }
        fs::write(path, bytes).unwrap_or_else(|error| panic!("write fixture: {error}"));
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) struct ScriptedServer {
    pub(super) base_url: String,
    worker: JoinHandle<Vec<serde_json::Value>>,
}

impl ScriptedServer {
    pub(super) fn start(responses: impl IntoIterator<Item = String>) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap_or_else(|error| panic!("bind fixture server: {error}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("fixture address: {error}"));
        listener
            .set_nonblocking(true)
            .unwrap_or_else(|error| panic!("nonblocking fixture listener: {error}"));
        let listener = tokio::net::TcpListener::from_std(listener)
            .unwrap_or_else(|error| panic!("async fixture listener: {error}"));
        let responses: Vec<_> = responses.into_iter().collect();
        let worker = tokio::spawn(async move {
            let mut requests = Vec::with_capacity(responses.len());
            for body in responses {
                let (mut stream, _) = listener
                    .accept()
                    .await
                    .unwrap_or_else(|error| panic!("accept fixture request: {error}"));
                let request = read_request(&mut stream).await;
                requests.push(request);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .await
                    .unwrap_or_else(|error| panic!("write fixture response: {error}"));
            }
            requests
        });
        Self {
            base_url: format!("http://{address}/v1"),
            worker,
        }
    }

    pub(super) async fn finish(mut self) -> Vec<serde_json::Value> {
        tokio::time::timeout(TIMEOUT, &mut self.worker)
            .await
            .unwrap_or_else(|_| panic!("fixture server timed out"))
            .unwrap_or_else(|error| panic!("fixture server task failed: {error}"))
    }
}

impl Drop for ScriptedServer {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> serde_json::Value {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let count = stream
            .read(&mut buffer)
            .await
            .unwrap_or_else(|error| panic!("read fixture request: {error}"));
        assert_ne!(count, 0, "request ended before its body");
        request.extend_from_slice(&buffer[..count]);
        assert!(
            request.len() <= 1024 * 1024,
            "fixture request exceeded bound"
        );
        let Some(header_end) = find_bytes(&request, b"\r\n\r\n") else {
            continue;
        };
        let body_start = header_end + 4;
        let headers = std::str::from_utf8(&request[..header_end])
            .unwrap_or_else(|error| panic!("fixture request headers: {error}"));
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or_else(|| panic!("fixture request has Content-Length"));
        let body_end = body_start.saturating_add(content_length);
        if request.len() >= body_end {
            return serde_json::from_slice(&request[body_start..body_end])
                .unwrap_or_else(|error| panic!("fixture request JSON: {error}"));
        }
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
