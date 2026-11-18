mod added;
mod check;
#[allow(dead_code)]
mod details;
mod doctor;
mod identity;
mod matcher;
mod public;
#[allow(dead_code)]
mod push;
#[allow(dead_code)]
mod repo;
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
    let advert = match public::probe(url) {
        Visibility::Private => return 0,
        Visibility::Public(ids) if !ids.is_empty() => Some(ids),
        Visibility::Unprobeable if !unprobeable_is_public(url) => return 0,
        _ => None,
    };
    let held = push::held(&repo, remote, url, advert);
    let details = details::discover(&details::machine());
    let m = matcher::Matcher::new(&details);
    let own: Vec<String> = details.emails.iter().map(|(e, _)| e.clone()).collect();
    let found = push::outgoing(&repo, &refs, &held)
        .and_then(|out| check::push_findings(&repo, &out, &refs, &m, &own))
        .unwrap_or_else(|e| fail(&format!("git failed reading the push ({e})")));
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

// A URL that can never be probed is asked about on every push, and nothing
// is saved. A failure to ask is checked as public.
fn unprobeable_is_public(url: &str) -> bool {
    tty::ask(&format!("Treat {url} as public? [Y/n]"), true).unwrap_or(true)
}

fn main() {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let arg = |i: usize| args.get(i).and_then(|a| a.to_str());
    match (arg(0), arg(1), arg(2)) {
        (Some("check-push"), Some(remote), Some(url)) => exit(check_push(remote, url)),
        (Some("doctor"), _, _) => exit(doctor::run()),
        _ => {
            eprintln!("usage: angerona check-push REMOTE URL | scan | doctor | setup");
            exit(2)
        }
    }
}
