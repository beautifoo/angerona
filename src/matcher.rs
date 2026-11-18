use crate::details::{Details, is_noreply};
use regex::{Captures, Regex};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Email,
    HomePath,
    Login,
    Host,
}

// `text` is the matched span as found, so a report can quote it.
#[derive(Debug, PartialEq)]
pub struct Hit {
    pub line: usize,
    pub kind: Kind,
    pub text: String,
}

// Built once, then asked about every line, path, identity and message.
pub struct Matcher {
    email_shape: Regex,
    email: Option<Regex>,
    home: Option<Regex>,
    login: Option<Regex>,
    host: Option<Regex>,
    login_name: Option<String>,
}

const LABEL_END: &str = "(?:[^A-Za-z0-9-]|$)";
const PATH_END: &str = r"(?:[^A-Za-z0-9._-]|\.(?:[^A-Za-z0-9_-]|$)|$)";
const SEP: &str = r"(?:/|\\{1,2}|%2F)";

// The span of the first group that took part in the match.
fn span(caps: &Captures) -> Option<(usize, usize)> {
    caps.iter()
        .skip(1)
        .flatten()
        .next()
        .map(|m| (m.start(), m.end()))
}

fn build(pattern: &str) -> Regex {
    Regex::new(&format!("(?i){pattern}")).expect("pattern is built from escaped details")
}

impl Matcher {
    // Escapes every detail, so a login with a dot or plus is still literal.
    pub fn new(d: &Details) -> Matcher {
        let mut home = vec![];
        if let Some(h) = &d.home {
            home.push(format!("({}){PATH_END}", regex::escape(h)));
        }
        if let Some(l) = &d.raw_login {
            let l = regex::escape(l);
            home.push(format!("((?:Users|home){SEP}+{l}){PATH_END}"));
            home.push(format!("(~{l}){SEP}"));
        }
        // Matched as written, so an apostrophe or accent in the address still counts. Whole
        // address only: no name character before it (a % is a space in a URL) and no
        // further domain label after it.
        let emails: Vec<_> = d.emails.iter().map(|(e, _)| regex::escape(e)).collect();
        let emails = if emails.is_empty() {
            vec![]
        } else {
            let all = emails.join("|");
            vec![format!(
                r"(?:^|[^A-Za-z0-9._+-]|%[0-9A-Fa-f]{{2}})({all})(?:[^A-Za-z0-9.-]|\.(?:[^A-Za-z0-9-]|$)|$)"
            )]
        };
        let mut login = vec![];
        let mut host = vec![];
        let hosts: Vec<_> = d.hosts.iter().map(|h| regex::escape(h)).collect();
        let hosts = hosts.join("|");
        if let Some(l) = &d.login {
            let l = regex::escape(l);
            login.push(format!(r"\b({l}\.xcuserdatad)"));
            if !hosts.is_empty() {
                login.push(format!(
                    r"\b({l}@(?:{hosts})(?:\.[A-Za-z0-9-]+)*){LABEL_END}"
                ));
            }
        }
        if !hosts.is_empty() {
            host.push(format!(r"(@(?:{hosts})(?:\.[A-Za-z0-9-]+)*){LABEL_END}"));
        }
        let join = |parts: Vec<String>| (!parts.is_empty()).then(|| build(&parts.join("|")));
        Matcher {
            email_shape: build(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+"),
            email: join(emails),
            home: join(home),
            login: join(login),
            host: join(host),
            login_name: d.login.clone(),
        }
    }

    // Every hit by position, each stretch of text reported once.
    pub fn find(&self, text: &str) -> Vec<Hit> {
        let mut found: Vec<(usize, usize, Kind)> = vec![];
        let mut noreply = vec![];
        for m in self.email_shape.find_iter(text) {
            // A full stop that ends a sentence is not part of the address.
            let s = m.as_str().trim_end_matches('.');
            if is_noreply(s) {
                noreply.push((m.start(), m.start() + s.len()));
            }
        }
        let unmasked = |(a, b): &(usize, usize)| !noreply.iter().any(|(x, y)| a < y && x < b);
        let rules = [
            (&self.email, Kind::Email, false),
            (&self.home, Kind::HomePath, false),
            (&self.login, Kind::Login, true),
            (&self.host, Kind::Host, true),
        ];
        for (re, kind, masked) in rules {
            let Some(re) = re else { continue };
            for caps in re.captures_iter(text) {
                let Some(s) = span(&caps) else { continue };
                if !masked || unmasked(&s) {
                    found.push((s.0, s.1, kind));
                }
            }
        }
        // Earliest first, longest first on a tie; drop what overlaps a kept hit.
        found.sort_by_key(|(a, b, _)| (*a, std::cmp::Reverse(*b)));
        let mut hits: Vec<Hit> = vec![];
        let mut end = 0;
        for (a, b, kind) in found {
            if a < end {
                continue;
            }
            end = b;
            hits.push(Hit {
                line: text[..a].matches('\n').count() + 1,
                kind,
                text: text[a..b].to_string(),
            });
        }
        hits
    }

    // A branch or tag named after you leaks you even with no home path in it.
    pub fn find_ref(&self, name: &str) -> Vec<Hit> {
        let mut hits = self.find(name);
        let bare = name
            .strip_prefix("refs/heads/")
            .or_else(|| name.strip_prefix("refs/tags/"))
            .unwrap_or(name);
        let first = bare.split('/').next().unwrap_or(bare);
        if self
            .login_name
            .as_deref()
            .is_some_and(|l| l.eq_ignore_ascii_case(first))
        {
            hits.push(Hit {
                line: 1,
                kind: Kind::Login,
                text: first.to_string(),
            });
        }
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matcher() -> Matcher {
        Matcher::new(&Details {
            emails: vec![("me@home.example".into(), "test".into())],
            home: Some("/Users/jdoe".into()),
            raw_login: Some("jdoe".into()),
            login: Some("jdoe".into()),
            hosts: vec!["jdoes-laptop".into()],
            weak: vec![],
        })
    }

    fn kinds(text: &str) -> Vec<Kind> {
        matcher().find(text).into_iter().map(|h| h.kind).collect()
    }

    #[test]
    fn finds_home_path_in_every_form() {
        let forms = [
            "/Users/jdoe/x",
            "/USERS/JDOE/x",
            r"\/Users\/jdoe\/x",
            r"C:\\Users\\jdoe\\x",
            "%2FUsers%2Fjdoe%2Fx",
            "file:///users/Jdoe/x",
            "~jdoe/x",
            "/home/jdoe/x",
        ];
        let got: Vec<_> = forms.iter().map(|f| kinds(f)).collect();
        assert_eq!(
            got,
            forms
                .iter()
                .map(|_| vec![Kind::HomePath])
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_longer_login_is_not_the_home_path() {
        assert_eq!(kinds("/Users/jdoeni/x"), vec![]);
    }

    #[test]
    fn login_only_after_a_home_root() {
        let got: Vec<_> = ["/dev/null", "jdoe wrote this", "src/jdoe/mod.rs"]
            .iter()
            .map(|t| kinds(t))
            .collect();
        assert_eq!(got, vec![vec![], vec![], vec![]]);
    }

    #[test]
    fn xcuserdata_names_the_login() {
        assert_eq!(
            kinds("App.xcodeproj/xcuserdata/jdoe.xcuserdatad/x"),
            vec![Kind::Login]
        );
    }

    #[test]
    fn prompt_text_is_found() {
        assert_eq!(kinds("jdoe@jdoes-laptop angerona %"), vec![Kind::Login]);
    }

    #[test]
    fn host_after_at_is_found() {
        assert_eq!(kinds("x@jdoes-laptop.local"), vec![Kind::Host]);
    }

    #[test]
    fn host_only_as_the_label_after_at() {
        let got: Vec<_> = ["https://jdoes-laptop.example.com", "security@ubuntu.com"]
            .iter()
            .map(|t| kinds(t))
            .collect();
        assert_eq!(got, vec![vec![], vec![]]);
    }

    #[test]
    fn own_email_matches_any_case() {
        assert_eq!(kinds("Me@Home.Example"), vec![Kind::Email]);
    }

    #[test]
    fn own_email_whole_only() {
        let got: Vec<_> = ["notme@home.example", "me@home.example.org"]
            .iter()
            .map(|t| kinds(t))
            .collect();
        assert_eq!(got, vec![vec![], vec![]]);
    }

    #[test]
    fn noreply_is_never_a_hit() {
        assert_eq!(kinds("123+jdoe@users.noreply.github.com"), vec![]);
    }

    #[test]
    fn ref_prefix_is_a_login() {
        assert_eq!(
            matcher().find_ref("refs/heads/jdoe/fix")[0].kind,
            Kind::Login
        );
    }

    #[test]
    fn ref_with_login_later_is_not_a_hit() {
        assert_eq!(matcher().find_ref("refs/heads/fix/jdoe-ui"), vec![]);
    }

    #[test]
    fn lines_are_counted() {
        assert_eq!(matcher().find("a\nb\n/Users/jdoe\n")[0].line, 3);
    }

    #[test]
    fn matches_around_invalid_utf8() {
        let text = String::from_utf8_lossy(b"caf\xe9\n/Users/jdoe\xff\n");
        let lines: Vec<_> = matcher().find(&text).iter().map(|h| h.line).collect();
        assert_eq!(lines, vec![2]);
    }

    fn with_emails(emails: &[&str]) -> Matcher {
        Matcher::new(&Details {
            emails: emails
                .iter()
                .map(|e| (e.to_string(), "test".into()))
                .collect(),
            home: None,
            raw_login: None,
            login: None,
            hosts: vec![],
            weak: vec![],
        })
    }

    #[test]
    fn email_with_an_apostrophe_is_found() {
        let hits = with_emails(&["o'brien@example.com"]).find("o'brien@example.com");
        assert_eq!(hits[0].text, "o'brien@example.com");
    }

    #[test]
    fn email_with_an_accent_is_found() {
        let hits = with_emails(&["jöhn@example.com"]).find("mail jöhn@example.com now");
        assert_eq!(hits[0].text, "jöhn@example.com");
    }

    #[test]
    fn url_encoded_email_is_found() {
        assert_eq!(kinds("Contact%20me@home.example"), vec![Kind::Email]);
    }

    #[test]
    fn email_before_a_sentence_end_is_found() {
        assert_eq!(kinds("write to me@home.example."), vec![Kind::Email]);
    }

    #[test]
    fn home_path_at_a_sentence_end_is_found() {
        assert_eq!(kinds("my files are in /Users/jdoe."), vec![Kind::HomePath]);
    }

    #[test]
    fn a_dotted_login_is_not_the_home_path() {
        assert_eq!(kinds("/Users/jdoe.smith/x"), vec![]);
    }

    #[test]
    fn json_escaped_tilde_form_is_found() {
        assert_eq!(kinds(r"~jdoe\/x"), vec![Kind::HomePath]);
    }
}
