//! JSONBin.io-backed [`Store`](super::Store).
//!
//! Storage is a single bin holding the JSON-encoded commits list. The wire
//! format mirrors the JSONBin.io v3 API: GETs return `{"record": [...]}`, PUTs
//! take the array directly. Header values may reference environment variables
//! (`$KEY` or `${KEY}`) which are expanded at request time so secrets stay out
//! of the on-disk config.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Deserialize;

use crate::commit::Commit;
use crate::error::{Error, Result};
use crate::repository::Repository;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// JSONBin.io response envelope. Only the `record` field is meaningful for us;
/// the API also returns `metadata`, which we don't store.
#[derive(Deserialize)]
struct JsonbinResponse {
    record: Vec<Commit>,
}

/// `Store` backed by a JSONBin.io bin URL.
pub struct JsonbinStore {
    url: String,
    headers: std::collections::BTreeMap<String, String>,
    /// Local cache file at `<managed>/.gitalong/commits.json`.
    cache_path: PathBuf,
    /// Touch-file used to gate re-pulls within the configured threshold.
    pull_marker: PathBuf,
    pull_threshold: f64,
}

impl JsonbinStore {
    /// Build a new JsonbinStore against the repository's config. Does not
    /// reach the network; the first read/write triggers it.
    pub fn new(repo: &Repository) -> Result<Self> {
        let cfg = repo.config();
        let cache_dir = repo.working_dir().join(".gitalong");
        std::fs::create_dir_all(&cache_dir)?;
        Ok(Self {
            url: cfg.store_url.clone(),
            headers: cfg.store_headers.clone(),
            cache_path: cache_dir.join("commits.json"),
            pull_marker: cache_dir.join(".pull"),
            pull_threshold: cfg.pull_threshold,
        })
    }

    /// Fetch the commits list, hitting the cache when within the pull window.
    pub fn read(&mut self) -> Result<Vec<Commit>> {
        if self.pulled_within_threshold() {
            return self.read_local();
        }
        match self.read_remote() {
            Ok(commits) => Ok(commits),
            // Best-effort fallback to cache when the network read fails so
            // local-only operations (status, claim hints) keep working
            // offline. Unreachable-store errors during write still bubble up.
            Err(Error::StoreUnreachable(_)) => self.read_local(),
            Err(e) => Err(e),
        }
    }

    /// Push the commits list to the bin, then update the local cache.
    pub fn write(&mut self, commits: &[Commit]) -> Result<()> {
        let body = serde_json::to_vec(commits)?;
        let response = ureq::put(&self.url)
            .config()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .build()
            .header("Content-Type", "application/json")
            .header_map(self.expanded_headers())
            .send(body.as_slice())
            .map_err(|e| Error::StoreUnreachable(format!("PUT {} failed: {e}", self.url)))?;

        let status = response.status();
        if !status.is_success() {
            return Err(Error::StoreUnreachable(format!(
                "PUT {} returned {status}",
                self.url
            )));
        }
        std::fs::write(&self.cache_path, body)?;
        Ok(())
    }

    fn read_remote(&self) -> Result<Vec<Commit>> {
        let mut response = ureq::get(&self.url)
            .config()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .build()
            .header_map(self.expanded_headers())
            .call()
            .map_err(|e| Error::StoreUnreachable(format!("GET {} failed: {e}", self.url)))?;

        let status = response.status();
        if !status.is_success() {
            return Err(Error::StoreUnreachable(format!(
                "GET {} returned {status}",
                self.url
            )));
        }
        let envelope: JsonbinResponse = response
            .body_mut()
            .read_json()
            .map_err(|e| Error::StoreUnreachable(format!("decoding GET {}: {e}", self.url)))?;
        // Refresh the cache so an offline next-read can answer.
        let body = serde_json::to_vec(&envelope.record)?;
        std::fs::write(&self.cache_path, body)?;
        touch(&self.pull_marker)?;
        Ok(envelope.record)
    }

    fn read_local(&self) -> Result<Vec<Commit>> {
        let bytes = match std::fs::read(&self.cache_path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        if bytes.is_empty() {
            return Ok(Vec::new());
        }
        let commits: Vec<Commit> = serde_json::from_slice(&bytes)?;
        Ok(commits)
    }

    fn pulled_within_threshold(&self) -> bool {
        match std::fs::metadata(&self.pull_marker).and_then(|m| m.modified()) {
            Ok(t) => SystemTime::now()
                .duration_since(t)
                .map(|d| d.as_secs_f64() < self.pull_threshold)
                .unwrap_or(false),
            Err(_) => false,
        }
    }

    /// Apply `$VAR` / `${VAR}` env-var expansion to each header value.
    fn expanded_headers(&self) -> std::collections::BTreeMap<String, String> {
        self.headers
            .iter()
            .map(|(k, v)| (k.clone(), expand_env(v)))
            .collect()
    }
}

/// Sugar for `ureq::Request` so we can apply a map of headers in one line.
trait HeaderMapExt {
    fn header_map(self, map: std::collections::BTreeMap<String, String>) -> Self;
}

impl HeaderMapExt for ureq::RequestBuilder<ureq::typestate::WithoutBody> {
    fn header_map(mut self, map: std::collections::BTreeMap<String, String>) -> Self {
        for (k, v) in map {
            self = self.header(k, v);
        }
        self
    }
}

impl HeaderMapExt for ureq::RequestBuilder<ureq::typestate::WithBody> {
    fn header_map(mut self, map: std::collections::BTreeMap<String, String>) -> Self {
        for (k, v) in map {
            self = self.header(k, v);
        }
        self
    }
}

/// Mirror of Python `os.path.expandvars`: `$NAME` or `${NAME}` is replaced by
/// `std::env::var("NAME")`; unset variables are left literal.
fn expand_env(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            // Handle ${NAME}
            if bytes[i + 1] == b'{'
                && let Some(close) = bytes[i + 2..].iter().position(|&b| b == b'}')
            {
                let name = &input[i + 2..i + 2 + close];
                match std::env::var(name) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => out.push_str(&input[i..i + 2 + close + 1]),
                }
                i += 2 + close + 1;
                continue;
            }
            // Handle $NAME (alphanumeric + underscore).
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            if end > start {
                let name = &input[start..end];
                match std::env::var(name) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => out.push_str(&input[i..end]),
                }
                i = end;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Update `path`'s mtime by writing an empty file (creating it if missing).
fn touch(path: &Path) -> Result<()> {
    std::fs::write(path, b"")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CONFIG_BASENAME, Config};
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    /// Stand up a managed git repo with a JSONBin.io-shaped store_url. The URL
    /// points at an unreachable host so the read path will fall back to the
    /// cache without us standing up an HTTP server.
    fn fixture(headers: BTreeMap<String, String>) -> (tempfile::TempDir, Repository) {
        let managed = tempdir().unwrap();
        git2::Repository::init(managed.path()).unwrap();
        let cfg = Config {
            // The 0.0.0.0 IP cannot route — guarantees a fast-fail on read.
            store_url: "https://api.jsonbin.io/0.0.0.0/never-resolves".into(),
            store_headers: headers,
            pull_threshold: 0.0,
            ..Config::default()
        };
        cfg.save(&managed.path().join(CONFIG_BASENAME)).unwrap();
        let repo = Repository::open(managed.path()).unwrap();
        (managed, repo)
    }

    #[test]
    fn read_falls_back_to_cached_commits_when_offline() {
        let (m, repo) = fixture(BTreeMap::new());
        let mut store = JsonbinStore::new(&repo).unwrap();

        // Seed a cache as if a previous successful read had stored commits.
        let cached = vec![Commit {
            sha: Some("cached".into()),
            ..Commit::default()
        }];
        std::fs::create_dir_all(m.path().join(".gitalong")).unwrap();
        std::fs::write(
            m.path().join(".gitalong/commits.json"),
            serde_json::to_vec(&cached).unwrap(),
        )
        .unwrap();

        let got = store.read().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].sha.as_deref(), Some("cached"));
    }

    #[test]
    fn read_returns_empty_vec_when_no_cache_exists_and_remote_is_unreachable() {
        let (_m, repo) = fixture(BTreeMap::new());
        let mut store = JsonbinStore::new(&repo).unwrap();
        // No cache, no network. read() should bail to an empty list rather
        // than failing — gitalong shouldn't blow up on offline machines.
        let got = store.read().unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn expand_env_substitutes_known_variables() {
        // Use a unique name to avoid clashing with whatever the test runner has.
        unsafe {
            std::env::set_var("GITALONG_TEST_VAR", "secret");
        }
        assert_eq!(expand_env("Bearer $GITALONG_TEST_VAR"), "Bearer secret");
        assert_eq!(expand_env("Bearer ${GITALONG_TEST_VAR}"), "Bearer secret");
    }

    #[test]
    fn expand_env_leaves_unknown_variables_literal() {
        unsafe {
            std::env::remove_var("GITALONG_DOES_NOT_EXIST_XYZ");
        }
        assert_eq!(
            expand_env("Bearer $GITALONG_DOES_NOT_EXIST_XYZ"),
            "Bearer $GITALONG_DOES_NOT_EXIST_XYZ"
        );
        assert_eq!(
            expand_env("Bearer ${GITALONG_DOES_NOT_EXIST_XYZ}"),
            "Bearer ${GITALONG_DOES_NOT_EXIST_XYZ}"
        );
    }

    #[test]
    fn expand_env_passes_strings_without_dollars_through() {
        assert_eq!(expand_env("plain text"), "plain text");
    }
}
