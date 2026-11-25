mod added;
mod check;
mod details;
mod doctor;
mod identity;
mod matcher;
mod public;
mod push;
mod repo;
mod scan;
mod setup;
mod text;
mod tty;
mod verdict;

use public::Visibility;
use std::{
    env,
    io::{self, Read},
    path::Path,
    process::exit,
};
use verdict::{Mode, Verdict};

fn fail(reason: &str) -> ! {
    eprintln!("angerona: {reason}, refusing to push");
    exit(1)
}

// The pre-push hook: exits 0 when the push may go, 1 after listing what in it
// names you. Any error reading the repo or the push refuses it.
fn check_push(remote: &str, url: &str) -> i32 {
    let mut lines = Vec::new();
    if io::stdin().read_to_end(&mut lines).is_err() {
        fail("can't read the refs to push");
    }
    let refs: Vec<_> = String::from_utf8_lossy(&lines)
        .lines()
        .filter_map(push::parse_ref_line)
        .collect();
    // A push of only deletions sends nothing: no need to ask.
    if refs.is_empty() {
        return 0;
    }
    // A partial clone must not fetch what it lacks: that is read as missing.
    // SAFETY: nothing else runs yet, so no thread reads the environment.
    unsafe { env::set_var("GIT_NO_LAZY_FETCH", "1") };
    let repo = repo::open(Path::new("."))
        .unwrap_or_else(|e| fail(&format!("git failed reading the push ({e})")));
    if repo.common_dir().join("info/grafts").exists() || env::var_os("GIT_GRAFT_FILE").is_some() {
        fail("this repo has grafts, which gix ignores");
    }
    // A URL that can't be probed, or a failure to ask, is checked as public.
    // A private one is still checked for the home path, login and machine
    // name, which mean nothing off this machine; emails only when public.
    let (public, advert) = match public::probe(url) {
        Visibility::Private => (false, None),
        Visibility::Public(ids) if !ids.is_empty() => (true, Some(ids)),
        Visibility::Unprobeable => (ask_if_public(url), None),
        _ => (true, None),
    };
    let held = push::held(&repo, remote, url, public, advert);
    let (m, v) = matcher_and_verdict(Mode::Push { public });
    let mut found = push::outgoing(&repo, &refs, &held)
        .and_then(|out| check::push_findings(&repo, &out, &refs, &m))
        .unwrap_or_else(|e| {
            // Only the range the push sends: git backfill alone fetches the
            // whole history HEAD reaches.
            if repo::partial(&repo) {
                eprintln!(
                    "angerona: this partial clone may lack what the push sends, and the hook \
                     never downloads it; fetch it first (git 2.55 or later): \
                     git backfill --no-sparse <remote>/<branch>..<commit>"
                );
            }
            fail(&format!("git failed reading the push ({e})"))
        });
    found.retain(|f| v.keeps(f));
    if found.is_empty() {
        return 0;
    }
    eprint!("angerona: this push names you:\n{}", check::render(&found));
    eprintln!("angerona: other hooks may still refuse this push");
    match tty::ask("Push anyway? [y/N]", false) {
        Some(true) => 0,
        Some(false) => 1,
        None => {
            eprintln!(
                "angerona: fix the lines above and push again; only a person should push past \
                 this, with git push --no-verify (it also skips every other pre-push hook)"
            );
            1
        }
    }
}

// The verdict on which email a finding may name in this mode, and a matcher
// for your details and the emails it says to look for.
fn matcher_and_verdict(mode: Mode) -> (matcher::Matcher, Verdict) {
    let details = details::discover(&details::machine());
    let v = Verdict::new(details::email_facts(&details), mode);
    (matcher::Matcher::new(&details, &v.emails_to_match()), v)
}

// A URL that can never be probed is asked about on every push, and nothing
// is saved. A failure to ask is checked as public. The URL is shown without
// its user and password.
fn ask_if_public(url: &str) -> bool {
    let shown = push::without_credentials(url);
    tty::ask(&format!("Treat {shown} as public? [Y/n]"), true).unwrap_or(true)
}

// angerona scan: run by hand in a repo before it goes public.
fn scan(all_emails: bool) -> i32 {
    let repo = match repo::open(Path::new(".")) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("angerona: {e}");
            return 1;
        }
    };
    let (m, v) = matcher_and_verdict(Mode::Scan { all_emails });
    scan::run(&repo, &m, &v)
}

const USAGE: &str =
    "usage: angerona check-push REMOTE URL | scan [--all-emails] | doctor | setup | --version";

// Exact arguments only, so a stray one, such as setup --help, never runs a
// command that writes your config.
fn main() {
    let args: Option<Vec<String>> = env::args_os()
        .skip(1)
        .map(|a| a.into_string().ok())
        .collect();
    let args = args.unwrap_or_default();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["check-push", remote, url] => exit(check_push(remote, url)),
        ["doctor"] => exit(doctor::run()),
        ["scan"] => exit(scan(false)),
        ["scan", "--all-emails"] => exit(scan(true)),
        ["setup"] => exit(setup::run()),
        ["-h" | "--help"] => println!("{USAGE}"),
        ["-V" | "--version"] => println!("angerona {}", env!("CARGO_PKG_VERSION")),
        _ => {
            eprintln!("{USAGE}");
            exit(2)
        }
    }
}
