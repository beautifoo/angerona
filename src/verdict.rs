use crate::{
    check::Finding,
    details::{is_address, is_noreply},
    matcher::Kind,
};

// The email git uses outside any repo: one, none set (git exits 1 and prints
// nothing), or unknown because git failed some other way.
pub enum Outside {
    Set(String),
    Unset,
    GitFailed,
}

// What decides which email a finding may name, read once by
// details::email_facts: your emails and raw login as discover found them, and
// what git says your email is here, all lowercased but the login.
pub struct EmailFacts {
    pub own: Vec<String>,
    pub repo_email: Option<String>,
    pub outside: Outside,
    pub login: Option<String>,
}

// What runs: the pre-push hook, to a public repo or not, or a scan, with or
// without --all-emails.
#[derive(Clone, Copy)]
pub enum Mode {
    Push { public: bool },
    Scan { all_emails: bool },
}

// The repo's own email: the one git uses in this repo, which that repo is
// meant to carry, when it is shaped like an address and differs from the one
// git uses outside any repo, which is never skipped. Any repo email differs
// from none set. A user.email of a bare login would hide the finding it
// equals, and when git failed outside, none is skipped, so more is reported,
// not less.
pub fn repo_own_email(f: &EmailFacts) -> Option<String> {
    let repo = f.repo_email.as_ref().filter(|e| is_address(e))?;
    match &f.outside {
        Outside::Set(o) if o == repo => None,
        Outside::Set(_) | Outside::Unset => Some(repo.clone()),
        Outside::GitFailed => None,
    }
}

// Whether an email looks guessed by a machine, not chosen by a person: a local
// domain (.local, .lan, .localdomain, .(none)) or none at all, which is git's
// login@host fallback. An empty address names nobody.
fn machine_guessed(email: &str) -> bool {
    let lower = email.to_lowercase();
    let Some((_, domain)) = lower.rsplit_once('@') else {
        return false;
    };
    !domain.is_empty()
        && (!domain.contains('.')
            || [".local", ".lan", ".localdomain", ".(none)"]
                .iter()
                .any(|s| domain.ends_with(s)))
}

// Decides, in one place, which email a finding may name.
pub struct Verdict {
    facts: EmailFacts,
    repo_own: Option<String>,
    mode: Mode,
}

impl Verdict {
    pub fn new(facts: EmailFacts, mode: Mode) -> Verdict {
        Verdict {
            repo_own: repo_own_email(&facts),
            facts,
            mode,
        }
    }

    // The emails to look for: none on a push to a repo that isn't public, which
    // may carry them, and never the repo's own.
    pub fn emails_to_match(&self) -> Vec<String> {
        if let Mode::Push { public: false } = self.mode {
            return Vec::new();
        }
        let mut emails = self.facts.own.clone();
        emails.retain(|e| Some(e) != self.repo_own.as_ref());
        emails
    }

    pub fn yours(&self, address: &str) -> bool {
        self.facts.own.contains(&address.to_lowercase())
    }

    // Whether a finding is reported. The repo's own email never is, by any
    // rule. A named address counts only as another person's, not yours (the
    // matcher looks for those) nor noreply, and only as the tagger or the
    // committer of an unpublished commit, the fields that say who ran git: so
    // fork syncs and patches you apply stay quiet. A scan marks every commit
    // unpublished. A push reports emails only to a public repo. Why a scan
    // narrows it: on a team repo, thousands of other people's emails bury the
    // findings, so it reports one only when it looks machine-guessed from your
    // login, your own old machine, or with --all-emails; an empty one never.
    pub fn keeps(&self, f: &Finding) -> bool {
        let address = f.address.as_deref().unwrap_or_default();
        if self.repo_own.as_deref() == Some(&address.to_lowercase()) {
            return false;
        }
        if f.named {
            let counts = f
                .ident
                .as_ref()
                .is_some_and(|(_, _, field)| field == "tagger")
                || f.unpublished;
            if !counts || is_noreply(address) || self.yours(address) {
                return false;
            }
        }
        match self.mode {
            Mode::Push { public } => public || f.kind != Kind::Email,
            Mode::Scan { all_emails } => {
                let local = address.rsplit_once('@').map(|(l, _)| l.to_lowercase());
                let guessed = machine_guessed(address)
                    && self.facts.login.as_deref().map(str::to_lowercase) == local;
                f.kind != Kind::Email
                    || (!address.is_empty() && (self.yours(address) || all_emails || guessed))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODES: [Mode; 4] = [
        Mode::Push { public: true },
        Mode::Push { public: false },
        Mode::Scan { all_emails: false },
        Mode::Scan { all_emails: true },
    ];

    fn facts() -> EmailFacts {
        EmailFacts {
            own: vec!["me@home.example".to_string(), "me@work.example".to_string()],
            repo_email: Some("me@work.example".to_string()),
            outside: Outside::Set("me@home.example".to_string()),
            login: Some("JDoe".to_string()),
        }
    }

    fn hit(kind: Kind, text: &str) -> Finding {
        Finding {
            at: "x".to_string(),
            what: "x".to_string(),
            hint: "",
            kind,
            address: (kind == Kind::Email || is_address(text)).then(|| text.to_string()),
            ident: None,
            unpublished: true,
            named: false,
        }
    }

    fn named(field: &str, address: &str, unpublished: bool) -> Finding {
        Finding {
            ident: Some((gix::objs::Kind::Commit, "0".to_string(), field.to_string())),
            unpublished,
            named: true,
            ..hit(Kind::Email, address)
        }
    }

    // Push to a public repo, push to one that isn't, scan, scan --all-emails.
    fn in_every_mode(f: &Finding) -> [bool; 4] {
        MODES.map(|m| Verdict::new(facts(), m).keeps(f))
    }

    fn own_email(repo: Option<&str>, outside: Outside) -> Option<String> {
        repo_own_email(&EmailFacts {
            own: vec![],
            repo_email: repo.map(str::to_string),
            outside,
            login: None,
        })
    }

    #[test]
    fn the_repo_email_is_its_own_only_when_it_differs_from_outside() {
        let set = |e: &str| Outside::Set(e.to_string());
        let got = [
            own_email(Some("me@work.example"), set("me@home.example")),
            own_email(Some("me@home.example"), set("me@home.example")),
            own_email(Some("me@work.example"), Outside::Unset),
            own_email(Some("me@work.example"), Outside::GitFailed),
            own_email(Some("jdoe"), Outside::Unset),
            own_email(None, Outside::Unset),
        ];
        assert_eq!(
            got,
            [
                Some("me@work.example".to_string()),
                None,
                Some("me@work.example".to_string()),
                None,
                None,
                None,
            ]
        );
    }

    #[test]
    fn your_emails_are_matched_except_the_repo_own_and_on_a_private_push() {
        let got = MODES.map(|m| Verdict::new(facts(), m).emails_to_match());
        let home = vec!["me@home.example".to_string()];
        assert_eq!(got, [home.clone(), vec![], home.clone(), home]);
    }

    #[test]
    fn your_email_is_kept_except_on_a_private_push() {
        let f = hit(Kind::Email, "Me@Home.Example");
        assert_eq!(in_every_mode(&f), [true, false, true, true]);
    }

    #[test]
    fn a_login_is_kept_everywhere() {
        let f = hit(Kind::Login, "jdoe.xcuserdatad");
        assert_eq!(in_every_mode(&f), [true; 4]);
    }

    #[test]
    fn the_repo_own_email_is_never_kept_by_any_rule() {
        let found = [
            hit(Kind::Login, "Me@Work.Example"),
            named("committer", "me@work.example", true),
        ];
        assert_eq!(found.map(|f| in_every_mode(&f)), [[false; 4]; 2]);
    }

    #[test]
    fn a_stranger_committer_of_unpublished_is_kept_on_a_public_push() {
        let f = named("committer", "bob@corp.example", true);
        assert_eq!(in_every_mode(&f), [true, false, false, true]);
    }

    // In a scan, only a tag's committer trailer is published: it marks every
    // commit unpublished.
    #[test]
    fn a_stranger_committer_of_published_is_never_kept() {
        let f = named("committer", "bob@corp.example", false);
        assert_eq!(in_every_mode(&f), [false; 4]);
    }

    #[test]
    fn a_stranger_tagger_is_kept_either_way() {
        let f = named("tagger", "bob@corp.example", false);
        assert_eq!(in_every_mode(&f), [true, false, false, true]);
    }

    #[test]
    fn a_named_noreply_or_own_address_is_never_kept() {
        let found = [
            named("committer", "1+bob@users.noreply.github.com", true),
            named("committer", "Me@Home.Example", true),
        ];
        assert_eq!(found.map(|f| in_every_mode(&f)), [[false; 4]; 2]);
    }

    // Your own old machine, whatever the case of your login.
    #[test]
    fn a_scan_keeps_an_address_guessed_from_your_login() {
        let f = named("committer", "jdoe@old-mac.local", true);
        assert_eq!(in_every_mode(&f), [true, false, true, true]);
    }

    #[test]
    fn a_scan_skips_a_colleague_guessed_address() {
        let f = named("committer", "pat@pats-laptop.local", true);
        assert_eq!(in_every_mode(&f), [true, false, false, true]);
    }

    #[test]
    fn a_scan_never_keeps_an_empty_address() {
        let f = named("committer", "", true);
        assert_eq!(in_every_mode(&f), [true, false, false, false]);
    }

    #[test]
    fn machine_guessed_emails() {
        for e in [
            "x@host.local",
            "x@host.lan",
            "x@host.localdomain",
            "x@host.(none)",
            "x@host",
        ] {
            assert!(machine_guessed(e), "{e}");
        }
    }

    #[test]
    fn chosen_and_empty_emails_are_not_machine_guessed() {
        for e in ["x@corp.example", "x@example.com", ""] {
            assert!(!machine_guessed(e), "{e:?}");
        }
    }
}
