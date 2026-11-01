//! Web frameworks `openworkers-core` converts requests and responses for.
//! Each one is behind a feature of the same name.

#[cfg(feature = "actix")]
pub mod actix;
#[cfg(feature = "axum")]
pub mod axum;
#[cfg(feature = "hyper")]
pub mod hyper;

#[cfg(any(feature = "actix", feature = "hyper"))]
use openworkers_core::HttpRequest;
#[cfg(any(feature = "actix", feature = "hyper"))]
use std::net::SocketAddr;

/// The scheme the client used. A backend cannot see TLS that a proxy ends in
/// front of it: behind such a proxy, the host says `Https`.
/// In actix, the backend finds it as `.app_data(Scheme::Https)`, not as
/// `web::Data<Scheme>`.
#[cfg(any(feature = "actix", feature = "hyper"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scheme {
    #[default]
    Http,
    Https,
}

#[cfg(any(feature = "actix", feature = "hyper"))]
impl Scheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Scheme::Http => "http",
            Scheme::Https => "https",
        }
    }
}

/// The connection a request came on, as the host knows it. The default has no
/// peer, so the worker gets no address, and the scheme `Http`.
#[cfg(any(feature = "actix", feature = "hyper"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Connection {
    /// The address of the client. Behind a proxy, this is the proxy.
    pub peer: Option<SocketAddr>,
    pub scheme: Scheme,
}

/// The URL the worker gets: the scheme the host trusts, the authority of the
/// request and its path with the query. `from_hyper_parts` and `from_actix`
/// of openworkers-core are not used for it: with no Host header (HTTP/2
/// carries `:authority`), the first gives `localhost` and the whole URI, and
/// the second reads the forwarded headers of the client.
#[cfg(any(feature = "actix", feature = "hyper"))]
pub(crate) fn url(scheme: Scheme, authority: &str, path_and_query: Option<&str>) -> String {
    format!(
        "{}://{authority}{}",
        scheme.as_str(),
        path_and_query.unwrap_or("/")
    )
}

/// `value` when it names a host as a URL can: a name of letters, digits, `.`,
/// `-` and `_`, or an IPv6 address in brackets, with an optional port. A
/// userinfo (`a@b`), a path, a query or any other character is refused: the
/// Host header goes into the URL of the worker.
#[cfg(any(feature = "actix", feature = "hyper"))]
pub(crate) fn checked_authority(value: &str) -> Option<&str> {
    let authority: http::uri::Authority = value.parse().ok()?;
    let host = authority.host();
    let named = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        && (!ends_in_number(host) || host.parse::<std::net::Ipv4Addr>().is_ok());
    let ipv6 = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .is_some_and(|address| address.parse::<std::net::Ipv6Addr>().is_ok());
    let whole = match authority.port() {
        Some(port) => port.as_str().parse::<u16>().is_ok() && value == format!("{host}:{port}"),
        None => value == host,
    };

    ((named || ipv6) && whole).then_some(value)
}

/// Whether the URL parser of the worker (WHATWG) reads `host` as an IPv4
/// address: its last label, after one trailing dot, is all digits or starts
/// with `0x`. Such a host that is not an IPv4 address (`1.2.3.4.5`,
/// `256.1.1.1`) makes `new URL` throw in the worker.
#[cfg(any(feature = "actix", feature = "hyper"))]
fn ends_in_number(host: &str) -> bool {
    let host = host.strip_suffix('.').unwrap_or(host);
    let last = host.rsplit('.').next().unwrap_or(host);

    (!last.is_empty() && last.bytes().all(|b| b.is_ascii_digit()))
        || last
            .get(..2)
            .is_some_and(|start| start.eq_ignore_ascii_case("0x"))
}

/// The headers that only the runner sets, as `RUNNER_HEADERS` of
/// openworkers-runner: a request from a client loses them.
#[cfg(any(feature = "actix", feature = "hyper"))]
const RUNNER_HEADERS: [&str; 12] = [
    "x-worker-id",
    "x-worker-name",
    "x-openworkers-internal",
    "x-openworkers-depth",
    "x-request-id",
    "forwarded",
    "x-forwarded-for",
    "x-forwarded-host",
    "x-forwarded-port",
    "x-forwarded-proto",
    "x-real-ip",
    "cf-connecting-ip",
];

/// The headers that carry the address of the client, as the runner sets them.
#[cfg(any(feature = "actix", feature = "hyper"))]
const CLIENT_HEADERS: [&str; 3] = ["x-real-ip", "x-forwarded-for", "cf-connecting-ip"];

/// Give the worker its client as the runner does: the headers of the runner
/// that the client sent are removed; the address of the peer goes in
/// `x-real-ip`, `x-forwarded-for` and `cf-connecting-ip`, and the scheme in
/// `x-forwarded-proto` (the backends start the URL with it, through [`url`]).
/// With no peer, the worker gets no address.
#[cfg(any(feature = "actix", feature = "hyper"))]
pub(crate) fn set_client(request: &mut HttpRequest, connection: Connection) {
    let scheme = connection.scheme.as_str();

    request.headers.retain(|name, _| {
        !RUNNER_HEADERS
            .iter()
            .any(|header| name.eq_ignore_ascii_case(header))
    });

    if let Some(peer) = connection.peer {
        let peer = peer.ip().to_canonical().to_string();

        for header in CLIENT_HEADERS {
            request.headers.insert(header.to_string(), peer.clone());
        }
    }

    request
        .headers
        .insert("x-forwarded-proto".to_string(), scheme.to_string());
}

#[cfg(all(test, any(feature = "actix", feature = "hyper")))]
mod tests {
    use super::checked_authority;

    #[test]
    fn an_authority_names_a_host_and_an_optional_port() {
        for good in [
            "app.test",
            "app.test:8080",
            "127.0.0.1",
            "127.0.0.1:3000",
            "[::1]",
            "[::1]:3000",
            "a_b-c",
            "1.2.3.example",
            "app0x.test",
        ] {
            assert_eq!(checked_authority(good), Some(good));
        }

        for bad in [
            "",
            "evil.example/admin?",
            "a?b",
            "a/b",
            "a@b",
            "user:pass@app.test",
            "a:",
            "a:99999",
            "a:8a",
            "a!b",
            "*",
            "a b",
            "[::1",
            "[app.test]",
            "1.2.3.4.5",
            "256.1.1.1",
            "app.1",
            "1.2.3.4.",
            "127.1",
            "app.0x10",
            "0x7f.0.0.1",
        ] {
            assert_eq!(checked_authority(bad), None, "{bad:?}");
        }
    }
}
