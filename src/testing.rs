//! Fixtures that stand up every store backend the same way, so one test body
//! runs unchanged against each of them. Only compiled for tests, behind the
//! `test-support` feature the crate's own dev-dependency turns on.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use tempfile::TempDir;

use crate::config::{CONFIG_BASENAME, Config};

/// The store backends a [`Team`] can be built on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreKind {
    Refs,
    Git,
    Jsonbin,
}

/// Expand one test body into a `#[test]` per store backend.
///
/// ```ignore
/// for_each_store!(publishes_something, |kind| {
///     let team = Team::new(kind);
///     // ...
/// });
/// ```
#[macro_export]
macro_rules! for_each_store {
    ($name:ident, $body:expr) => {
        mod $name {
            #[allow(unused_imports)]
            use super::*;

            #[test]
            fn refs_store() {
                ($body)($crate::testing::StoreKind::Refs)
            }

            #[test]
            fn git_store() {
                ($body)($crate::testing::StoreKind::Git)
            }

            #[test]
            fn jsonbin_store() {
                ($body)($crate::testing::StoreKind::Jsonbin)
            }
        }
    };
}

/// A bare origin plus whatever the chosen store needs behind it.
pub struct Team {
    kind: StoreKind,
    origin: TempDir,
    store: Option<TempDir>,
    jsonbin: Option<FakeJsonbin>,
}

impl Team {
    pub fn new(kind: StoreKind) -> Self {
        let origin = tempfile::tempdir().unwrap();
        git(
            origin.path(),
            &["init", "--quiet", "--bare", "--initial-branch=main"],
        );
        let (store, jsonbin) = match kind {
            StoreKind::Refs => (None, None),
            StoreKind::Git => {
                let store = tempfile::tempdir().unwrap();
                git(
                    store.path(),
                    &["init", "--quiet", "--bare", "--initial-branch=main"],
                );
                (Some(store), None)
            }
            StoreKind::Jsonbin => (None, Some(FakeJsonbin::start())),
        };
        Self {
            kind,
            origin,
            store,
            jsonbin,
        }
    }

    pub fn kind(&self) -> StoreKind {
        self.kind
    }

    /// Path of the bare origin, usable as a clone URL.
    pub fn origin(&self) -> &Path {
        self.origin.path()
    }

    /// What `.gitalong.json` needs in `store_url`. Empty for the refs store.
    pub fn store_url(&self) -> String {
        match self.kind {
            StoreKind::Refs => String::new(),
            StoreKind::Git => format!("file://{}", self.store.as_ref().unwrap().path().display()),
            StoreKind::Jsonbin => self.jsonbin.as_ref().unwrap().url(),
        }
    }

    /// Config pointing at this team's store, pulling on every read.
    pub fn config(&self) -> Config {
        Config {
            store_url: self.store_url(),
            pull_threshold: 0.0,
            ..Config::default()
        }
    }

    /// Arguments for `gitalong setup` that select this team's store.
    pub fn setup_args(&self) -> Vec<String> {
        let mut args = vec!["setup".to_string()];
        if self.kind != StoreKind::Refs {
            args.push(self.store_url());
        }
        args.extend(["--pull-threshold".to_string(), "0".to_string()]);
        args
    }

    /// Origin addressed as a `file://` URL, a different spelling of the same
    /// repository than the plain path [`clone`](Self::clone) uses.
    pub fn origin_file_url(&self) -> String {
        format!("file://{}", self.origin.path().display())
    }

    /// Clone origin with `name` as the committer identity and the store
    /// config plus the gitignore patch written but not committed.
    pub fn clone(&self, name: &str, configure: impl FnOnce(&mut Config)) -> TempDir {
        let url = self.origin.path().to_str().unwrap().to_string();
        self.clone_via(name, &url, configure)
    }

    /// [`clone`](Self::clone), addressing origin as `url` so tests can mix
    /// spellings of one origin the way a real team does.
    pub fn clone_via(&self, name: &str, url: &str, configure: impl FnOnce(&mut Config)) -> TempDir {
        let clone = tempfile::tempdir().unwrap();
        git(
            clone.path(),
            &["clone", "--quiet", url, clone.path().to_str().unwrap()],
        );
        git(clone.path(), &["config", "user.name", name]);
        let email = format!("{}@example.com", name.to_lowercase());
        git(clone.path(), &["config", "user.email", &email]);
        let mut config = self.config();
        configure(&mut config);
        config.save(&clone.path().join(CONFIG_BASENAME)).unwrap();
        std::fs::write(
            clone.path().join(".gitignore"),
            crate::hooks::GITIGNORE_PATCH,
        )
        .unwrap();
        clone
    }

    /// [`clone`](Self::clone), then commit the config, the gitignore patch
    /// and `files` and push them as `main`, so later clones start from there.
    pub fn seeded_clone(
        &self,
        name: &str,
        files: &[(&str, &str)],
        configure: impl FnOnce(&mut Config),
    ) -> TempDir {
        let clone = self.clone(name, configure);
        for (path, body) in files {
            std::fs::write(clone.path().join(path), body).unwrap();
        }
        git(clone.path(), &["add", "--all"]);
        git(clone.path(), &["commit", "--quiet", "-m", "init"]);
        git(clone.path(), &["push", "--quiet", "-u", "origin", "main"]);
        clone
    }
}

/// Run `git <args>` in `dir`, panic with stderr on failure, return stdout.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {} in {} failed: {}",
        args.join(" "),
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A JSONBin.io lookalike holding one bin: `GET` returns `{"record": ...}`,
/// `PUT` replaces it. Listens on a loopback port until dropped.
pub struct FakeJsonbin {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl FakeJsonbin {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let record = Arc::new(Mutex::new(b"[]".to_vec()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = stop.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Ok(stream) = stream {
                        serve(stream, &record);
                    }
                }
            })
        };
        Self {
            addr,
            stop,
            thread: Some(thread),
        }
    }

    pub fn url(&self) -> String {
        format!("http://{}/v3/b/bin", self.addr)
    }
}

impl Drop for FakeJsonbin {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Answer one HTTP/1.1 request on `stream` and close it.
fn serve(stream: TcpStream, record: &Mutex<Vec<u8>>) {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
        return;
    }
    let method = request_line
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();

    let mut content_length = 0usize;
    let mut chunked = false;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim_end().is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "content-length" => content_length = value.trim().parse().unwrap_or(0),
                "transfer-encoding" => chunked = value.trim().eq_ignore_ascii_case("chunked"),
                _ => {}
            }
        }
    }
    let body = if chunked {
        read_chunked(&mut reader)
    } else {
        let mut body = vec![0; content_length];
        let _ = reader.read_exact(&mut body);
        body
    };

    let response = {
        let mut stored = record.lock().unwrap();
        if method == "PUT" {
            *stored = body;
        }
        let mut out = b"{\"record\":".to_vec();
        out.extend_from_slice(&stored);
        out.extend_from_slice(b",\"metadata\":{}}");
        out
    };
    let mut stream = reader.into_inner();
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.len()
    );
    let _ = stream.write_all(&response);
    let _ = stream.flush();
}

fn read_chunked(reader: &mut impl BufRead) -> Vec<u8> {
    let mut body = Vec::new();
    loop {
        let mut size_line = String::new();
        if reader.read_line(&mut size_line).unwrap_or(0) == 0 {
            break;
        }
        let size = size_line.trim().split(';').next().unwrap_or("0");
        let size = usize::from_str_radix(size, 16).unwrap_or(0);
        if size == 0 {
            break;
        }
        let mut chunk = vec![0; size];
        if reader.read_exact(&mut chunk).is_err() {
            break;
        }
        body.extend_from_slice(&chunk);
        let mut crlf = String::new();
        let _ = reader.read_line(&mut crlf);
    }
    body
}
