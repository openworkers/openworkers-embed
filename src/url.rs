/// The path of a URL or of a bare path: no scheme, host, query or fragment.
pub(crate) fn path(url: &str) -> &str {
    let rest = match url.split_once("://") {
        Some((_, rest)) => rest.find('/').map_or("/", |start| &rest[start..]),
        None => url,
    };

    rest.split(['?', '#']).next().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_scheme_host_query_and_fragment() {
        assert_eq!(path("http://localhost/a/b?x=1#top"), "/a/b");
        assert_eq!(path("http://localhost"), "/");
        assert_eq!(path("/a/b.js"), "/a/b.js");
    }
}
