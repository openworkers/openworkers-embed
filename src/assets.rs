//! Static files served from a directory. The names of the files are indexed
//! once, at start, as sirv does in production, so a page route such as `/`
//! is answered from memory, with no system call.
//!
//! When the files change on disk after start:
//! - A file that changes its content is served with the new content: the
//!   content is read on each request.
//! - A path whose last segment has a dot, such as `/_app/immutable/x.js`, is
//!   looked for on disk when the index misses it. So the files a rebuild adds,
//!   or a directory made after start, are served.
//! - A prerendered page added after start (`/about` for `about.html`) is not
//!   served until a restart: the request goes to the worker.
//! - A file removed after start is not served: its read fails.
//!
//! A name that starts with a dot is never served, except `/.well-known/`.
//!
//! Known limits:
//! - A directory that two links reach is indexed under the first one only.
//!   Through the other, a name with a dot is served by the disk fallback, and
//!   a prerendered page is not.
//! - Links are followed and not checked against the root: a link to `/`
//!   indexes and serves that whole tree. Do not put such a link in the
//!   assets directory.
//! - The dot rule reads the names of the URL, not the target of a link: a
//!   link named `env.txt` to `.env` serves it. Only the owner of the
//!   directory can make such a link.
//! - On Windows, a short 8.3 name (`ENVLOC~1.LOC` for `.env.local`) can reach
//!   a hidden file through the disk fallback.
//! - The index compares names with their case. On a file system
//!   that ignores case, as macOS does by default, `/Favicon.png` does not find
//!   `favicon.png`, unless the last segment has a dot and the disk finds it.

use bytes::Bytes;
use openworkers_core::{HttpResponse, ResponseBody};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

pub(crate) struct Assets {
    root: PathBuf,
    files: HashSet<String>,
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
        let files = index(&root);

        Self { root, files }
    }

    /// The file for a URL path: the exact file, then a prerendered page
    /// (`/about` is `about.html` or `about/index.html`).
    pub async fn get(&self, path: &str) -> Option<Asset> {
        let path = normalize(path)?;

        let candidates = if path.is_empty() {
            vec!["index.html".to_string()]
        } else {
            vec![
                path.clone(),
                format!("{path}.html"),
                format!("{path}/index.html"),
            ]
        };

        // A last segment with a dot names a file, which a rebuild may have added
        let file = path
            .rsplit('/')
            .next()
            .is_some_and(|last| last.contains('.'));

        for candidate in candidates {
            if !self.files.contains(&candidate) && !(file && candidate == path) {
                continue;
            }

            if let Some(body) = read_file(self.root.join(&candidate)).await {
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

/// The content of a regular file. A FIFO, a device or a link to one is not
/// read: it could block a thread of the blocking pool, or never end.
async fn read_file(path: PathBuf) -> Option<Vec<u8>> {
    tokio::task::spawn_blocking(move || {
        if !std::fs::metadata(&path).ok()?.is_file() {
            return None;
        }

        std::fs::read(&path).ok()
    })
    .await
    .ok()
    .flatten()
}

/// The path relative to the root, with `/` between its components: `//` and
/// `/./` are folded. `None` for a path with `..`, a backslash, a component that
/// is not a plain name, or a hidden name (see `hidden`).
fn normalize(path: &str) -> Option<String> {
    let path = path.trim_start_matches('/');

    if path.contains('\\') {
        return None;
    }

    let mut out = String::with_capacity(path.len());

    for component in Path::new(path).components() {
        let name = match component {
            Component::Normal(name) => name.to_str()?,
            // A leading ./ (std keeps it; it folds the others)
            Component::CurDir => continue,
            _ => return None,
        };

        if hidden(name, out.is_empty()) {
            return None;
        }

        if !out.is_empty() {
            out.push('/');
        }

        out.push_str(name);
    }

    Some(out)
}

/// A name that starts with a dot, such as `.env` or `.git`, is never served, as
/// sirv does by default. Only `.well-known` at the root is served.
fn hidden(name: &str, at_root: bool) -> bool {
    name.starts_with('.') && !(at_root && name == ".well-known")
}

/// The files under `root`, as paths relative to it with `/` between
/// components. Links are followed. Each directory is walked once, by its
/// canonical path: a loop of links ends, and many links to one directory cost
/// no more than one. All the real directories of the tree are walked before
/// any link, so a directory that a link also reaches keeps its own name. The
/// files of an alias are not in the index; a name with a dot is still served
/// through it by the disk fallback. A directory that cannot be read adds
/// nothing.
fn index(root: &Path) -> HashSet<String> {
    let mut files = HashSet::new();
    let mut walked = HashSet::new();
    let mut links = std::collections::VecDeque::new();

    walk(root, "", &mut files, &mut walked, &mut links);

    while let Some((directory, prefix)) = links.pop_front() {
        walk(&directory, &prefix, &mut files, &mut walked, &mut links);
    }

    files
}

/// Walks the real directories under `directory`; the links to directories
/// wait in `links` until every real directory is walked.
fn walk(
    directory: &Path,
    prefix: &str,
    files: &mut HashSet<String>,
    walked: &mut HashSet<PathBuf>,
    links: &mut std::collections::VecDeque<(PathBuf, String)>,
) {
    let Ok(canonical) = std::fs::canonicalize(directory) else {
        return;
    };

    if !walked.insert(canonical.clone()) {
        return;
    }

    // Read through the canonical path: a long chain of links would pass the
    // limit of links in one path (32 on macOS)
    let Ok(entries) = std::fs::read_dir(&canonical) else {
        return;
    };

    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };

        if hidden(&name, prefix.is_empty()) {
            continue;
        }

        let Ok(metadata) = std::fs::metadata(entry.path()) else {
            continue;
        };
        let path = format!("{prefix}{name}");

        if metadata.is_file() {
            files.insert(path);
        } else if metadata.is_dir() {
            if entry.file_type().is_ok_and(|kind| kind.is_symlink()) {
                links.push_back((entry.path(), format!("{path}/")));
            } else {
                walk(&entry.path(), &format!("{path}/"), files, walked, links);
            }
        }
    }
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

    fn write(root: &Path, path: &str, body: &str) {
        let path = root.join(path);

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    async fn body(assets: &Assets, path: &str) -> Option<Vec<u8>> {
        assets.get(path).await.map(|asset| asset.body.to_vec())
    }

    #[test]
    fn parent_directories_are_refused() {
        assert_eq!(
            normalize("/_app/immutable/a.js").unwrap(),
            "_app/immutable/a.js"
        );
        assert!(normalize("../secret").is_none());
        assert!(normalize("/a/../../secret").is_none());
        assert!(normalize("/a\\b").is_none());
    }

    #[tokio::test]
    async fn hidden_files_are_not_served() {
        let root = tempfile::tempdir().unwrap();

        write(root.path(), ".git/config", "secret");
        write(root.path(), "a/.hidden.js", "secret");
        write(root.path(), ".well-known/security.txt", "contact");
        write(root.path(), ".well-known/.x", "secret");
        write(root.path(), "a/.well-known/x", "secret");

        let assets = Assets::new(root.path().to_path_buf());

        // Made after start, with a dot in the last segment: the disk is not read
        write(root.path(), ".env", "secret");

        assert!(body(&assets, "/.env").await.is_none());
        assert!(body(&assets, "/.git/config").await.is_none());
        assert!(body(&assets, "/a/.hidden.js").await.is_none());
        assert!(body(&assets, "/a/../.env").await.is_none());
        assert!(body(&assets, "/.well-known/.x").await.is_none());
        assert!(body(&assets, "/a/.well-known/x").await.is_none());
        assert!(!assets.files.iter().any(|file| file.contains(".git")));
        assert_eq!(
            body(&assets, "/.well-known/security.txt").await.unwrap(),
            b"contact"
        );
    }

    #[test]
    fn repeated_slashes_and_dots_fold() {
        assert_eq!(normalize("/./a.js").unwrap(), "a.js");
        assert_eq!(normalize("/_app//a.js").unwrap(), "_app/a.js");
        assert_eq!(normalize("/a/./b.js").unwrap(), "a/b.js");
        assert_eq!(normalize("/about/").unwrap(), "about");
        assert_eq!(normalize("/").unwrap(), "");
    }

    #[tokio::test]
    async fn files_are_indexed_at_start() {
        let root = tempfile::tempdir().unwrap();

        write(root.path(), "index.html", "home");
        write(root.path(), "about.html", "about");
        write(root.path(), "_app/immutable/a.js", "a");

        let assets = Assets::new(root.path().to_path_buf());

        assert_eq!(body(&assets, "/").await.unwrap(), b"home");
        assert_eq!(body(&assets, "/about").await.unwrap(), b"about");
        assert_eq!(body(&assets, "/_app//immutable/a.js").await.unwrap(), b"a");
        assert_eq!(body(&assets, "/_app/./immutable/a.js").await.unwrap(), b"a");
        assert!(body(&assets, "/missing").await.is_none());

        // A change of content shows
        write(root.path(), "index.html", "new home");

        assert_eq!(body(&assets, "/").await.unwrap(), b"new home");

        // A removed file is not served
        std::fs::remove_file(root.path().join("_app/immutable/a.js")).unwrap();

        assert!(body(&assets, "/_app/immutable/a.js").await.is_none());
    }

    #[tokio::test]
    async fn a_rebuild_serves_its_new_files() {
        let root = tempfile::tempdir().unwrap();

        write(root.path(), "index.html", "v1");

        let assets = Assets::new(root.path().to_path_buf());

        // vite build writes new hashed chunks next to a new index.html
        write(root.path(), "index.html", "v2");
        write(root.path(), "_app/immutable/chunks/new.js", "chunk");

        assert_eq!(body(&assets, "/").await.unwrap(), b"v2");
        assert_eq!(
            body(&assets, "/_app/immutable/chunks/new.js")
                .await
                .unwrap(),
            b"chunk"
        );

        // A prerendered page added after start waits for a restart
        write(root.path(), "late.html", "late");

        assert!(body(&assets, "/late").await.is_none());
    }

    #[tokio::test]
    async fn a_root_made_after_start_serves_its_files() {
        let site = tempfile::tempdir().unwrap();
        let root = site.path().join("assets");
        let assets = Assets::new(root.clone());

        write(&root, "_app/immutable/a.js", "a");

        assert_eq!(body(&assets, "/_app/immutable/a.js").await.unwrap(), b"a");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn linked_directories_are_followed_once() {
        let site = tempfile::tempdir().unwrap();
        let root = site.path().join("client");

        write(site.path(), "static/robots.txt", "robots");
        std::fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(site.path().join("static"), root.join("static")).unwrap();

        // A link back to the root: the index must end
        std::os::unix::fs::symlink(&root, root.join("loop")).unwrap();

        let assets = Assets::new(root);

        assert!(assets.files.contains("static/robots.txt"));
        assert_eq!(
            body(&assets, "/static/robots.txt").await.unwrap(),
            b"robots"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_alias_of_a_directory_serves_its_files_from_disk() {
        let root = tempfile::tempdir().unwrap();

        write(root.path(), "a/x.js", "x");
        std::os::unix::fs::symlink(root.path().join("a"), root.path().join("b")).unwrap();

        let assets = Assets::new(root.path().to_path_buf());

        // One of the two is in the index; both serve x.js
        assert_eq!(assets.files.len(), 1);
        assert_eq!(body(&assets, "/a/x.js").await.unwrap(), b"x");
        assert_eq!(body(&assets, "/b/x.js").await.unwrap(), b"x");
    }

    // A link to .well-known at the root: the real directory keeps its name
    #[cfg(unix)]
    #[tokio::test]
    async fn a_link_to_well_known_does_not_hide_it() {
        let root = tempfile::tempdir().unwrap();

        write(
            root.path(),
            ".well-known/apple-app-site-association",
            "apps",
        );
        std::os::unix::fs::symlink(root.path().join(".well-known"), root.path().join("aa"))
            .unwrap();

        let assets = Assets::new(root.path().to_path_buf());

        assert_eq!(
            body(&assets, "/.well-known/apple-app-site-association")
                .await
                .unwrap(),
            b"apps"
        );
    }

    // Links to a real directory from directories that read_dir may give
    // first: the real directory keeps its own name in the index, so its
    // prerendered page (no dot, so no disk fallback) is found
    #[cfg(unix)]
    #[tokio::test]
    async fn a_real_directory_wins_over_links_to_it() {
        let root = tempfile::tempdir().unwrap();

        write(root.path(), "zzz/real/page.html", "page");

        for name in ["a", "b", "c", "d", "e", "f", "g", "h"] {
            std::fs::create_dir_all(root.path().join(name)).unwrap();
            std::os::unix::fs::symlink(
                root.path().join("zzz/real"),
                root.path().join(name).join("l"),
            )
            .unwrap();
        }

        let assets = Assets::new(root.path().to_path_buf());

        assert_eq!(body(&assets, "/zzz/real/page").await.unwrap(), b"page");
    }

    // Two links per level to the next directory: each directory is walked once
    #[cfg(unix)]
    #[test]
    fn many_links_to_one_directory_cost_one_walk() {
        let root = tempfile::tempdir().unwrap();
        let depth = 18;

        for level in 0..depth {
            let here = root.path().join(format!("d{level}"));
            let next = root.path().join(format!("d{}", level + 1));

            std::fs::create_dir_all(&here).unwrap();
            std::fs::create_dir_all(&next).unwrap();
            std::os::unix::fs::symlink(&next, here.join("a")).unwrap();
            std::os::unix::fs::symlink(&next, here.join("b")).unwrap();
        }

        write(root.path(), &format!("d{depth}/x.js"), "x");

        let start = std::time::Instant::now();
        let assets = Assets::new(root.path().join("d0"));
        let elapsed = start.elapsed();

        eprintln!("index of {depth} levels with 2 links each: {elapsed:?}");
        assert_eq!(assets.files.len(), 1);
        assert!(elapsed < std::time::Duration::from_secs(1), "{elapsed:?}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_fifo_is_not_read() {
        let root = tempfile::tempdir().unwrap();
        let fifo = root.path().join("pipe.bin");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();

        assert!(made.success());

        let assets = Assets::new(root.path().to_path_buf());
        let read = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            body(&assets, "/pipe.bin"),
        );

        assert!(read.await.expect("the read of a FIFO blocked").is_none());
    }

    #[test]
    fn only_hashed_files_are_immutable() {
        assert!(immutable("_app/immutable/chunks/a.js").is_some());
        assert!(immutable("favicon.ico").is_none());
    }
}
