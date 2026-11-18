use std::{
    process::{Command, Stdio},
    time::Duration,
};

use curl::easy::{Easy, NetRc};
use gix::{ObjectId, url::Scheme};

use crate::push::without_credentials;

const ADVERTISEMENT: &str = "application/x-git-upload-pack-advertisement";

#[derive(Debug, PartialEq)]
pub enum Visibility {
    Public(Vec<ObjectId>),
    Private,
    Unprobeable,
    Passing,
}

pub enum Answer {
    Http {
        status: u32,
        content_type: String,
        body: Vec<u8>,
    },
    Timeout,
    Dns,
    Refused,
    Tls,
    Other,
}

// A path or file:// URL cannot be public, and has no host to ask.
pub fn is_local(url: &str) -> bool {
    if url.starts_with("file://") {
        return true;
    }
    if url.contains("://") {
        return false;
    }
    // scp-like: a colon before the first slash.
    let head = url.split('/').next().unwrap_or(url);
    !head.contains(':')
}

// ssh and git URLs name the same repo over https on the host ssh would
// reach, so the probe can ask without any login.
pub fn https_form(url: &str, resolve: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let parsed = gix::url::parse(url).ok()?;
    let host = parsed.host_argument_safe()?;
    let path = parsed.path.to_string();
    let path = path.trim_start_matches('/');
    match parsed.scheme {
        Scheme::Https | Scheme::Http => Some(without_credentials(url)),
        Scheme::Ssh => {
            let host = resolve(host)?;
            let host = if host == "ssh.github.com" {
                "github.com"
            } else {
                &host
            };
            Some(format!("https://{host}/{path}"))
        }
        Scheme::Git => Some(format!("https://{host}/{path}")),
        _ => None,
    }
}

// The hostname ssh would connect to for this alias.
fn ssh_hostname(host: &str) -> Option<String> {
    let out = Command::new("ssh")
        .args(["-G", "--", host])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("hostname "))
        .map(str::to_string)
}

// What the server said decides whether the push is checked: only a repo
// anyone can read is public, and a failure to ask never blocks a push.
pub fn classify(a: Answer) -> Visibility {
    match a {
        Answer::Http {
            status: 200,
            content_type,
            body,
        } if content_type.starts_with(ADVERTISEMENT) => Visibility::Public(advertised(&body)),
        Answer::Http {
            status: 401 | 403 | 404,
            ..
        } => Visibility::Private,
        Answer::Http {
            status: 429 | 500..=599,
            ..
        } => Visibility::Passing,
        Answer::Http { .. } | Answer::Refused | Answer::Tls => Visibility::Unprobeable,
        Answer::Timeout | Answer::Dns | Answer::Other => Visibility::Passing,
    }
}

// The commits the public side already has: every ref line starts with its
// id, peeled tags included.
pub fn advertised(body: &[u8]) -> Vec<ObjectId> {
    let mut ids = Vec::new();
    let mut rest = body;
    while rest.len() >= 4 {
        let Some(len) = std::str::from_utf8(&rest[..4])
            .ok()
            .and_then(|h| usize::from_str_radix(h, 16).ok())
        else {
            break;
        };
        if len < 4 {
            rest = &rest[4..];
            continue;
        }
        if len > rest.len() {
            break;
        }
        let line = &rest[4..len];
        rest = &rest[len..];
        if line.starts_with(b"# service=") {
            continue;
        }
        let hex = line.iter().take_while(|b| b.is_ascii_hexdigit()).count();
        if (hex == 40 || hex == 64)
            && let Ok(id) = ObjectId::from_hex(&line[..hex])
        {
            ids.push(id);
        }
    }
    ids
}

// Firewalls in front of some forges refuse a request with no User-Agent, or
// one that is not git's, with a 403, which would read as private. It names
// only the program, no credential.
fn user_agent() -> String {
    format!("git/angerona-{}", env!("CARGO_PKG_VERSION"))
}

// One GET with nothing of ours in it: no netrc, no auth, no headers but the
// User-Agent, and the URL without its user and password.
fn get(url: &str) -> Answer {
    let mut body = Vec::new();
    let mut status = 0;
    let mut content_type = String::new();
    let mut easy = Easy::new();
    let result = (|| {
        easy.url(&format!("{url}/info/refs?service=git-upload-pack"))?;
        easy.follow_location(true)?;
        easy.connect_timeout(Duration::from_secs(3))?;
        easy.timeout(Duration::from_secs(5))?;
        easy.netrc(NetRc::Ignored)?;
        easy.useragent(&user_agent())?;
        let mut transfer = easy.transfer();
        // Each redirect sends its own headers, so the last ones win.
        transfer.header_function(|line| {
            let line = String::from_utf8_lossy(line);
            if line.starts_with("HTTP/") {
                status = line
                    .split(' ')
                    .nth(1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                content_type.clear();
            } else if let Some((name, value)) = line.split_once(':')
                && name.eq_ignore_ascii_case("content-type")
            {
                content_type = value.trim().to_string();
            }
            true
        })?;
        transfer.write_function(|data| {
            body.extend_from_slice(data);
            Ok(data.len())
        })?;
        transfer.perform()
    })();
    match result {
        Ok(()) => Answer::Http {
            status,
            content_type,
            body,
        },
        Err(e) if e.is_operation_timedout() => timed_out(status, &content_type),
        Err(e) if e.is_couldnt_resolve_host() => Answer::Dns,
        Err(e) if e.is_couldnt_connect() => Answer::Refused,
        Err(e)
            if e.is_ssl_connect_error()
                || e.is_ssl_certproblem()
                || e.is_ssl_cacert()
                || e.is_ssl_cipher()
                || e.is_ssl_issuer_error()
                || e.is_peer_failed_verification() =>
        {
            Answer::Tls
        }
        Err(_) => Answer::Other,
    }
}

// A timeout after the advertisement's headers still means the repo is
// public; the body just did not arrive, so the caller lists the refs
// another way.
fn timed_out(status: u32, content_type: &str) -> Answer {
    if status == 200 && content_type.starts_with(ADVERTISEMENT) {
        Answer::Http {
            status,
            content_type: content_type.to_string(),
            body: Vec::new(),
        }
    } else {
        Answer::Timeout
    }
}

// The four answers a test can ask for.
#[cfg(any(debug_assertions, test))]
fn forced(value: &str) -> Option<Visibility> {
    match value {
        "public" => Some(Visibility::Public(Vec::new())),
        "private" => Some(Visibility::Private),
        "unprobeable" => Some(Visibility::Unprobeable),
        "passing" => Some(Visibility::Passing),
        _ => None,
    }
}

// Debug builds let the tests say what a remote is, so bats can push to
// local bare repos as if they were public.
pub fn probe(url: &str) -> Visibility {
    #[cfg(debug_assertions)]
    if let Some(v) = std::env::var("ANGERONA_TEST_VISIBILITY")
        .ok()
        .and_then(|v| forced(&v))
    {
        return v;
    }
    if is_local(url) {
        return Visibility::Private;
    }
    match https_form(url, &ssh_hostname) {
        Some(https) => classify(get(&https)),
        None => Visibility::Unprobeable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "1111111111111111111111111111111111111111";
    const B: &str = "2222222222222222222222222222222222222222";
    const C: &str = "3333333333333333333333333333333333333333";
    const ADVERT: &str = ADVERTISEMENT;

    fn resolve(h: &str) -> Option<String> {
        Some(if h == "github-work" { "github.com" } else { h }.to_string())
    }

    fn form(url: &str) -> Option<String> {
        https_form(url, &resolve)
    }

    fn http(status: u32, content_type: &str) -> Answer {
        Answer::Http {
            status,
            content_type: content_type.to_string(),
            body: Vec::new(),
        }
    }

    fn pkt(s: &str) -> String {
        format!("{:04x}{s}", s.len() + 4)
    }

    #[test]
    fn https_form_maps_scp_like() {
        assert_eq!(
            form("git@github-work:o/r.git").as_deref(),
            Some("https://github.com/o/r.git")
        );
    }

    #[test]
    fn https_form_maps_ssh_github_port_443() {
        assert_eq!(
            form("ssh://git@ssh.github.com:443/o/r.git").as_deref(),
            Some("https://github.com/o/r.git")
        );
    }

    #[test]
    fn https_form_maps_ssh_with_port() {
        assert_eq!(
            form("ssh://git@gitea.example:2222/o/r").as_deref(),
            Some("https://gitea.example/o/r")
        );
    }

    #[test]
    fn https_form_maps_git() {
        assert_eq!(
            form("git://host/r.git").as_deref(),
            Some("https://host/r.git")
        );
    }

    #[test]
    fn https_form_drops_credentials() {
        assert_eq!(
            form("https://me:tok@github.com/o/r.git").as_deref(),
            Some("https://github.com/o/r.git")
        );
    }

    #[test]
    fn https_form_keeps_http() {
        assert_eq!(
            form("http://host/r.git").as_deref(),
            Some("http://host/r.git")
        );
    }

    #[test]
    fn local_path_is_local() {
        assert!(is_local("/srv/r.git"));
    }

    #[test]
    fn relative_path_is_local() {
        assert!(is_local("../r.git"));
    }

    #[test]
    fn file_url_is_local() {
        assert!(is_local("file:///srv/r.git"));
    }

    #[test]
    fn scp_like_is_not_local() {
        assert!(!is_local("git@github.com:o/r.git"));
    }

    #[test]
    fn classifies_advertisement_as_public() {
        let body = pkt(&format!("{A} refs/heads/main\n")).into_bytes();
        let answer = Answer::Http {
            status: 200,
            content_type: ADVERT.to_string(),
            body,
        };
        assert_eq!(
            classify(answer),
            Visibility::Public(vec![gix::ObjectId::from_hex(A.as_bytes()).unwrap()])
        );
    }

    #[test]
    fn classifies_401_as_private() {
        assert_eq!(classify(http(401, "")), Visibility::Private);
    }

    #[test]
    fn classifies_403_as_private() {
        assert_eq!(classify(http(403, "")), Visibility::Private);
    }

    #[test]
    fn classifies_404_as_private() {
        assert_eq!(classify(http(404, "")), Visibility::Private);
    }

    #[test]
    fn classifies_refused_as_unprobeable() {
        assert_eq!(classify(Answer::Refused), Visibility::Unprobeable);
    }

    #[test]
    fn classifies_tls_as_unprobeable() {
        assert_eq!(classify(Answer::Tls), Visibility::Unprobeable);
    }

    #[test]
    fn classifies_html_200_as_unprobeable() {
        assert_eq!(classify(http(200, "text/html")), Visibility::Unprobeable);
    }

    #[test]
    fn classifies_203_as_unprobeable() {
        assert_eq!(classify(http(203, "text/html")), Visibility::Unprobeable);
    }

    #[test]
    fn classifies_leftover_redirect_as_unprobeable() {
        assert_eq!(classify(http(302, "")), Visibility::Unprobeable);
    }

    #[test]
    fn classifies_timeout_as_passing() {
        assert_eq!(classify(Answer::Timeout), Visibility::Passing);
    }

    #[test]
    fn classifies_dns_as_passing() {
        assert_eq!(classify(Answer::Dns), Visibility::Passing);
    }

    #[test]
    fn classifies_other_as_passing() {
        assert_eq!(classify(Answer::Other), Visibility::Passing);
    }

    #[test]
    fn classifies_503_as_passing() {
        assert_eq!(classify(http(503, "")), Visibility::Passing);
    }

    #[test]
    fn classifies_429_as_passing() {
        assert_eq!(classify(http(429, "")), Visibility::Passing);
    }

    #[test]
    fn reads_an_advertisement() {
        let body = format!(
            "{}0000{}{}{}0000",
            pkt("# service=git-upload-pack\n"),
            pkt(&format!("{A} HEAD\0multi_ack side-band\n")),
            pkt(&format!("{B} refs/tags/v1\n")),
            pkt(&format!("{C} refs/tags/v1^{{}}\n")),
        );
        let ids: Vec<_> = [A, B, C]
            .iter()
            .map(|h| gix::ObjectId::from_hex(h.as_bytes()).unwrap())
            .collect();
        assert_eq!(advertised(body.as_bytes()), ids);
    }

    #[test]
    fn https_form_refuses_an_option_looking_host() {
        assert_eq!(form("ssh://-oProxyCommand=x/r"), None);
    }

    #[test]
    fn the_user_agent_reads_as_git() {
        assert!(user_agent().starts_with("git/"));
    }

    #[test]
    fn seam_public() {
        assert_eq!(forced("public"), Some(Visibility::Public(Vec::new())));
    }

    #[test]
    fn seam_private() {
        assert_eq!(forced("private"), Some(Visibility::Private));
    }

    #[test]
    fn seam_unprobeable() {
        assert_eq!(forced("unprobeable"), Some(Visibility::Unprobeable));
    }

    #[test]
    fn seam_passing() {
        assert_eq!(forced("passing"), Some(Visibility::Passing));
    }

    #[test]
    fn seam_ignores_other_values() {
        assert_eq!(forced("whatever"), None);
    }

    #[test]
    fn timeout_after_advertisement_headers_is_public() {
        assert_eq!(
            classify(timed_out(200, ADVERT)),
            Visibility::Public(Vec::new())
        );
    }

    #[test]
    fn timeout_after_other_headers_is_passing() {
        assert_eq!(classify(timed_out(200, "text/html")), Visibility::Passing);
    }

    #[test]
    fn timeout_before_headers_is_passing() {
        assert_eq!(classify(timed_out(0, "")), Visibility::Passing);
    }
    #[test]
    #[ignore]
    fn network_public() {
        let v = probe("https://github.com/rust-lang/rust.git");
        assert!(matches!(v, Visibility::Public(ids) if !ids.is_empty()));
    }

    #[test]
    #[ignore]
    fn network_private() {
        assert_eq!(
            probe("https://github.com/beautifoo/no-such-repo-angerona.git"),
            Visibility::Private
        );
    }
}
