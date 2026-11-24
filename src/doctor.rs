use crate::{details, setup, verdict};
use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

const NEW_GIT: &str = "install git 2.54 or later, e.g. brew install git";

fn executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

// The path with the home folder written as ~, so doctor's output can be
// pasted somewhere public.
fn tilde(path: &str) -> String {
    let home = env::var("HOME").unwrap_or_default();
    let home = home.trim_end_matches('/');
    match path.strip_prefix(home).filter(|rest| rest.starts_with('/')) {
        Some(rest) if !home.is_empty() => format!("~{rest}"),
        _ => path.to_string(),
    }
}

// The value of one global git config key; None when unset or git can't run.
fn git_global(key: &str) -> Option<String> {
    let out = Command::new("git")
        .args(["config", "--global", "--get", key])
        .output()
        .ok()?;
    let value = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !value.is_empty()).then_some(value)
}

// A global boolean as git reads it, with yes, on or 1 for true, and set in an
// included file as well; false when unset.
fn git_global_true(key: &str) -> bool {
    Command::new("git")
        .args([
            "config",
            "--global",
            "--includes",
            "--type=bool",
            "--get",
            key,
        ])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
}

// The addresses the emails file holds, lowercased and sorted; None when the
// file is missing.
#[cfg(feature = "github-emails")]
fn saved_emails() -> Option<Vec<String>> {
    let text = fs::read_to_string(details::emails_path()?).ok()?;
    let mut emails: Vec<String> = text
        .lines()
        .map(|l| l.trim().to_lowercase())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    emails.sort();
    emails.dedup();
    Some(emails)
}

// Checks the git settings that keep your details out of pushes and prints each
// problem with the command that fixes it. Exits 1 when there is a problem;
// notes alone don't fail.
pub fn run() -> i32 {
    let mut problems = String::new();
    let mut notes = String::new();
    check_gits(&mut problems, &mut notes);

    if !git_global_true("user.useConfigOnly") {
        problems += "user.useConfigOnly is not true, so git guesses an email from your host name; fix: git config set --global user.useConfigOnly true\n";
    }

    match git_global("user.email") {
        Some(email) if !details::is_noreply(&email) => {
            problems += &format!(
                "the global user.email {email} is a real address; fix: set it to your noreply form <id>+<user>@users.noreply.github.com (one per host, see includeIf)\n"
            );
        }
        Some(_) => {}
        None => {
            notes += "no global user.email: with useConfigOnly, git refuses to commit in a repo that sets none; set one per host in an includeIf file as <id>+<user>@users.noreply.github.com\n";
        }
    }

    let includes = Command::new("git")
        .args(["config", "--global", "--get-regexp", "^includeif\\."])
        .output()
        .is_ok_and(|o| o.status.success() && !o.stdout.is_empty());
    if !includes {
        notes += "no includeIf in your global git config; give each host its own email with includeIf \"hasconfig:remote.*.url:git@<host>:*/**\" and a path to a file that sets user.email\n";
    }

    let hook = git_global("hook.angerona.command") == Some(setup::hook_command())
        && git_global("hook.angerona.event").as_deref() == Some("pre-push");
    if !hook {
        problems += "the pre-push hook is missing or changed (hook.angerona.command, hook.angerona.event); fix: angerona setup\n";
    }

    // The hook line's own lookup, under the PATH this shell gives it.
    let found = Command::new("/bin/sh")
        .args(["-c", "command -v angerona"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|p| !p.is_empty());
    if found.is_none() {
        problems += "angerona is not on your PATH, so the hook refuses every push; fix: add the folder that holds angerona to your PATH\n";
    }

    // A per-host includeIf file is where a signing key usually lives. A
    // key:: value is an SSH public key in full, whose user@host comment an
    // SSH signature doesn't carry.
    let machine = details::machine();
    for (key, file) in details::config_values(&machine.config_files, "user.signingkey") {
        if key.contains('@') && !key.starts_with("key::") {
            problems += &format!(
                "user.signingkey in {file} looks like an email, which signs commits with it as the key name; fix: set it to the key fingerprint\n"
            );
        }
    }

    #[cfg(feature = "github-emails")]
    let saved = saved_emails();
    #[cfg(feature = "github-emails")]
    match setup::gh_emails() {
        setup::Gh::Emails(mut now) => {
            now.sort();
            if saved.as_ref() != Some(&now) {
                problems += "the emails file is missing or differs from your GitHub account's addresses; fix: angerona setup\n";
            }
        }
        setup::Gh::Unavailable => {
            notes +=
                "gh is not available, so the emails file can't be checked against your account\n";
        }
        setup::Gh::NeedsScope => {
            notes += "gh lacks the user:email scope, so the emails file can't be checked; run: gh auth refresh -h github.com -s user:email\n";
        }
        setup::Gh::Failed(first) => {
            notes += &format!(
                "gh could not list your emails ({first}), so the emails file can't be checked\n"
            );
        }
    }

    let details = details::discover(&machine);
    for line in &details.weak {
        notes += line;
        if cfg!(target_os = "macos") && line.starts_with("host ") {
            notes += "; fix: sudo scutil --set HostName <name>";
        }
        notes += "\n";
    }

    for line in problems.lines() {
        println!("angerona: problem: {line}");
    }
    for line in notes.lines() {
        println!("angerona: note: {line}");
    }
    if problems.is_empty() {
        println!("angerona: no problems found");
    }
    if let Some(path) = found {
        println!("angerona: the hook finds angerona at {}", tilde(&path));
    }

    // The repo's own email is never reported here, not even as a committer, so
    // the list says so, and adds it when only the repo sets it. A noreply one
    // is left out, as the list leaves out every noreply address.
    const OWN_HERE: &str = "this repo's own, never reported here";
    let own_here = verdict::repo_own_email(&details::email_facts(&details))
        .filter(|e| !details::is_noreply(e));
    println!("angerona: emails kept out of pushes:");
    for (email, place) in &details.emails {
        if own_here.as_ref() == Some(email) {
            println!("  {email} ({place}; {OWN_HERE})");
        } else {
            println!("  {email} ({place})");
        }
    }
    if let Some(e) = own_here.filter(|e| !details.emails.iter().any(|(x, _)| x == e)) {
        println!("  {e} ({OWN_HERE})");
    }
    println!(
        "angerona: note: turn on your forge's email privacy setting (GitHub: Keep my email addresses private), since commits made on the website skip the hook"
    );
    println!("angerona: repos already public are not rechecked; run angerona scan in each");
    i32::from(!problems.is_empty())
}

// A git before 2.54, or one that can't run, skips config hooks without a
// word. The first git on PATH is the one that runs them here, so it adds to
// problems; another adds only to notes, since an app may still commit with
// it.
fn check_gits(problems: &mut String, notes: &mut String) {
    let gits = gits_on_path();
    let Some(first) = gits.first() else {
        *problems += &format!("no git on PATH; fix: {NEW_GIT}\n");
        return;
    };

    let mut good = None;
    let mut bad = Vec::new();
    for g in &gits {
        let version = version_line(g);
        if new_enough(&version) {
            if good.is_none() {
                good = Some(g);
            }
        } else {
            bad.push((g, version));
        }
    }

    let dir = |g: &Path| g.parent().unwrap_or(g).display().to_string();
    for (g, version) in bad {
        let fix = match good {
            None => NEW_GIT.to_string(),
            Some(good) if g == first => format!("put {} before {} on PATH", dir(good), dir(g)),
            Some(good) => format!("point any app that uses it at {}", good.display()),
        };
        let msg = format!(
            "{} ({version}) skips config hooks, so a commit or push made with it is not checked; fix: {fix}\n",
            g.display()
        );
        if g == first {
            *problems += &msg;
        } else {
            *notes += &msg;
        }
    }
}

// Every git on PATH, each once, in PATH order.
fn gits_on_path() -> Vec<PathBuf> {
    let mut gits: Vec<PathBuf> = Vec::new();
    let path = env::var_os("PATH").unwrap_or_default();
    for dir in env::split_paths(&path).filter(|d| !d.as_os_str().is_empty()) {
        let g = dir.join("git");
        if executable(&g) && !gits.contains(&g) {
            gits.push(g);
        }
    }
    gits
}

// Why the first git on PATH, the one that runs config hooks here, can't run
// the hook; None when it can.
pub fn git_skips_hooks() -> Option<String> {
    let Some(first) = gits_on_path().into_iter().next() else {
        return Some("there is no git on PATH".to_string());
    };
    let version = version_line(&first);
    (!new_enough(&version)).then(|| format!("{} ({version}) skips config hooks", first.display()))
}

// The first line git --version prints on stdout, past any warning, or on
// stderr when there is none; nothing when it can't run.
fn version_line(git: &Path) -> String {
    let Ok(out) = Command::new(git).arg("--version").output() else {
        return String::new();
    };
    let first_line = |b: &[u8]| {
        String::from_utf8_lossy(b)
            .lines()
            .next()
            .unwrap_or("")
            .to_string()
    };
    let line = first_line(&out.stdout);
    if line.is_empty() {
        first_line(&out.stderr)
    } else {
        line
    }
}

// Whether a "git version 2.54.0" line names 2.54 or later.
fn new_enough(version: &str) -> bool {
    let parts: Vec<&str> = version.split([' ', '.']).collect();
    let number = |i: usize| -> u32 {
        let part = parts.get(i).copied().unwrap_or("");
        let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().unwrap_or(0)
    };
    let major = number(2);
    let minor = number(3);
    major > 2 || (major == 2 && minor >= 54)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_git_version_as_2_54_or_later() {
        for v in [
            "git version 2.54.0",
            "git version 2.56.1",
            "git version 3.0.0",
            "git version 2.54.0.windows.1",
        ] {
            assert!(new_enough(v), "{v}");
        }
        for v in [
            "git version 2.53.0",
            "git version 2.39.5 (Apple Git-154)",
            "warning: x",
            "",
        ] {
            assert!(!new_enough(v), "{v}");
        }
    }
}
