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

use public::Visibility;
use std::{
    env,
    ffi::OsString,
    io::{self, Read},
    path::Path,
    process::exit,
};

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
    let held = push::held(&repo, remote, url, advert);
    let (mut details, skip) = details_here();
    if !public {
        details.emails.clear();
    }
    let m = matcher::Matcher::new(&details);
    let own: Vec<String> = details.emails.iter().map(|(e, _)| e.clone()).collect();
    let mut found = push::outgoing(&repo, &refs, &held)
        .and_then(|out| check::push_findings(&repo, &out, &refs, &m, &own))
        .unwrap_or_else(|e| fail(&format!("git failed reading the push ({e})")));
    found.retain(|f| !check::quotes_any(f, &skip) && (public || !f.what.starts_with("email ")));
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
                "angerona: no terminal to ask; to push once past this: git push --no-verify \
                 (it also skips every other pre-push hook)"
            );
            1
        }
    }
}

// Your details, less the addresses never reported in this repo, which come
// back too, to drop from what another rule finds.
fn details_here() -> (details::Details, Vec<String>) {
    let mut details = details::discover(&details::machine());
    let skip = details::skipped_here();
    details.emails.retain(|(e, _)| !skip.contains(e));
    (details, skip)
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
    let (details, skip) = details_here();
    let m = matcher::Matcher::new(&details);
    let own: Vec<String> = details.emails.iter().map(|(e, _)| e.clone()).collect();
    scan::run(
        &repo,
        &m,
        &own,
        &skip,
        details.raw_login.as_deref(),
        all_emails,
    )
}

fn main() {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let arg = |i: usize| args.get(i).and_then(|a| a.to_str());
    match (arg(0), arg(1), arg(2)) {
        (Some("check-push"), Some(remote), Some(url)) => exit(check_push(remote, url)),
        (Some("doctor"), _, _) => exit(doctor::run()),
        (Some("scan"), None, _) => exit(scan(false)),
        (Some("scan"), Some("--all-emails"), None) => exit(scan(true)),
        (Some("setup"), _, _) => exit(setup::run()),
        _ => {
            eprintln!(
                "usage: angerona check-push REMOTE URL | scan [--all-emails] | doctor | setup"
            );
            exit(2)
        }
    }
}
