//! Static files served from a directory, read on every request so a rebuilt
//! bundle shows up without a restart.

use bytes::Bytes;
use openworkers_core::{HttpResponse, ResponseBody};
use std::path::{Component, Path, PathBuf};

pub(crate) struct Assets {
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
        Self { root }
    }

    /// The file for a URL path: the exact file, then a prerendered page
    /// (`/about` is `about.html` or `about/index.html`).
    pub async fn get(&self, path: &str) -> Option<Asset> {
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

    #[test]
    fn only_hashed_files_are_immutable() {
        assert!(immutable("_app/immutable/chunks/a.js").is_some());
        assert!(immutable("favicon.ico").is_none());
    }
}
