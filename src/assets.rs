//! Static files served from a directory, read on every request so a rebuilt
//! bundle shows up without a restart. A path that names no file is
//! remembered for a second, so a new file shows up within a second.

use bytes::Bytes;
use openworkers_core::{HttpResponse, ResponseBody};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// How long a path that names no file is remembered. A file that a rebuild
/// adds shows up after this.
const MISS_TTL: Duration = Duration::from_secs(1);

/// The misses a thread remembers at most, so that a scan of random paths
/// cannot grow the set.
const MAX_MISSES: usize = 4096;

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// Most requests name no file, and a miss costs up to three stat calls.
    /// Each thread remembers its misses, by directory and path.
    static MISSES: RefCell<Misses> = RefCell::new(Misses {
        since: Instant::now(),
        paths: HashSet::new(),
    });
}

struct Misses {
    since: Instant,
    paths: HashSet<(usize, String)>,
}

impl Misses {
    /// Forgets every miss once they are older than `MISS_TTL`.
    fn fresh(&mut self) -> &mut HashSet<(usize, String)> {
        if self.since.elapsed() > MISS_TTL || self.paths.len() >= MAX_MISSES {
            self.paths.clear();
            self.since = Instant::now();
        }

        &mut self.paths
    }
}

pub(crate) struct Assets {
    id: usize,
    root: PathBuf,
}

pub(crate) struct Asset {
    body: Bytes,
    content_type: &'static str,
    cache_control: Option<&'static str>,
}

impl Asset {
    pub fn into_http(self) -> HttpResponse {
        let mut headers = vec![("content-type".to_string(), self.content_type.to_string())];

        if let Some(cache_control) = self.cache_control {
            headers.push(("cache-control".to_string(), cache_control.to_string()));
        }

        HttpResponse {
            status: 200,
            headers,
            body: ResponseBody::Bytes(self.body),
        }
    }
}

impl Assets {
    pub fn new(root: PathBuf) -> Self {
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            root,
        }
    }

    /// The file for a URL path: the exact file, then a prerendered page
    /// (`/about` is `about.html` or `about/index.html`).
    pub async fn get(&self, path: &str) -> Option<Asset> {
        let key = (self.id, path.to_string());

        if MISSES.with(|misses| misses.borrow_mut().fresh().contains(&key)) {
            return None;
        }

        let asset = self.find(path).await;

        if asset.is_none() {
            MISSES.with(|misses| misses.borrow_mut().fresh().insert(key));
        }

        asset
    }

    async fn find(&self, path: &str) -> Option<Asset> {
        let path = path.trim_start_matches('/');

        let candidates = if path.is_empty() {
            vec!["index.html".to_string()]
        } else {
            let path = path.trim_end_matches('/');

            vec![
                path.to_string(),
                format!("{path}.html"),
                format!("{path}/index.html"),
            ]
        };

        for candidate in candidates {
            if !is_plain(&candidate) {
                return None;
            }

            let file = self.root.join(&candidate);

            // Most requests name no file. tokio::fs would send each miss to a
            // blocking thread and back, so a stat on this thread answers it.
            if !std::fs::metadata(&file).is_ok_and(|metadata| metadata.is_file()) {
                continue;
            }

            if let Ok(body) = tokio::fs::read(file).await {
                return Some(Asset {
                    content_type: content_type(&candidate),
                    cache_control: immutable(&candidate),
                    body: body.into(),
                });
            }
        }

        None
    }
}

/// Only normal components: no `..`, no root, no drive prefix, no backslash.
fn is_plain(path: &str) -> bool {
    !path.contains('\\')
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

/// SvelteKit names these files by hash, so they never change.
fn immutable(path: &str) -> Option<&'static str> {
    path.contains("_app/immutable/")
        .then_some("public, max-age=31536000, immutable")
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "txt" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "wasm" => "application/wasm",
        "webmanifest" => "application/manifest+json",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_directories_are_refused() {
        assert!(is_plain("_app/immutable/a.js"));
        assert!(!is_plain("../secret"));
        assert!(!is_plain("a/../../secret"));
        assert!(!is_plain("a\\b"));
    }

    #[tokio::test]
    async fn a_miss_is_remembered_for_a_second() {
        let root = tempfile::tempdir().unwrap();
        let assets = Assets::new(root.path().to_path_buf());

        assert!(assets.get("/new.txt").await.is_none());

        std::fs::write(root.path().join("new.txt"), "here").unwrap();
        assert!(assets.get("/new.txt").await.is_none());

        tokio::time::sleep(MISS_TTL + Duration::from_millis(100)).await;
        assert!(assets.get("/new.txt").await.is_some());
    }

    #[test]
    fn only_hashed_files_are_immutable() {
        assert!(immutable("_app/immutable/chunks/a.js").is_some());
        assert!(immutable("favicon.ico").is_none());
    }
}
