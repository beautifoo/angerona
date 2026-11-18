use crate::details;
use std::{
    fs::{self, DirBuilder, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::Path,
    process::Command,
};

// Git appends "$@" to a config hook's command, so the trailing f gets the
// remote and URL. Without angerona on PATH the push goes through with a note.
pub const HOOK_COMMAND: &str = "f() { if command -v angerona >/dev/null 2>&1; then angerona check-push \"$@\"; else echo 'angerona not installed, push not checked' >&2; fi; }; f";

// Sets one global config key; git follows a symlinked config file.
fn git_set(key: &str, value: &str) -> Result<(), String> {
    let out = Command::new("git")
        .args(["config", "set", "--global", key, value])
        .output()
        .map_err(|e| format!("can't run git ({e})"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

// Writes the lines to the path through a temp file in the same directory, so
// a reader never sees half of them. The directory is 700, the file 600.
fn write_private(path: &Path, lines: &[String]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    let tmp = dir.join(format!(".emails.{}", std::process::id()));
    let _ = fs::remove_file(&tmp);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    let mut text = lines.join("\n");
    text.push('\n');
    let written = file.write_all(text.as_bytes());
    drop(file);
    match written.and_then(|_| fs::rename(&tmp, path)) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

// What asking gh for the account's emails came to.
pub enum Gh {
    Emails(Vec<String>),
    Unavailable,
    NeedsScope,
    Failed(String),
}

// Asks gh for the account's addresses, lowercased, without noreply ones or
// repeats. Shared by setup, which saves them, and doctor, which compares.
pub fn gh_emails() -> Gh {
    let Ok(out) = Command::new("gh")
        .args(["api", "user/emails", "--jq", ".[].email"])
        .output()
    else {
        return Gh::Unavailable;
    };
    if out.status.success() {
        let mut emails: Vec<String> = Vec::new();
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let email = line.trim().to_lowercase();
            if !email.is_empty() && !details::is_noreply(&email) && !emails.contains(&email) {
                emails.push(email);
            }
        }
        return Gh::Emails(emails);
    }
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if said.contains("scope") {
        Gh::NeedsScope
    } else if said.contains("gh auth login") || said.contains("not logged") {
        Gh::Unavailable
    } else {
        Gh::Failed(said.lines().next().unwrap_or("").trim().to_string())
    }
}

// Saves the account's emails for the hook to read. Never fails setup: each
// way gh can fall short prints one line and leaves any existing file.
fn save_emails() {
    match gh_emails() {
        Gh::Emails(emails) => {
            let Some(path) = details::emails_path() else {
                return println!("angerona: no config directory to save your emails in");
            };
            match write_private(&path, &emails) {
                Ok(()) => println!(
                    "angerona: saved {} emails to {}",
                    emails.len(),
                    path.display()
                ),
                Err(e) => println!("angerona: could not save your emails ({e})"),
            }
        }
        Gh::Unavailable => {
            println!("gh not available: only the emails in your git config are known")
        }
        Gh::NeedsScope => {
            println!("run: gh auth refresh -h github.com -s user:email, then angerona setup again")
        }
        Gh::Failed(first) => {
            println!("angerona: gh could not list your emails ({first}), left them as they were")
        }
    }
}

// Writes the pre-push hook into the global git config, then saves the emails
// of your GitHub account. Exits 1 only when the hook could not be written.
pub fn run() -> i32 {
    let set = git_set("hook.angerona.event", "pre-push")
        .and_then(|_| git_set("hook.angerona.command", HOOK_COMMAND));
    if let Err(e) = set {
        eprintln!("angerona: could not write the hook: {e}");
        return 1;
    }
    println!(
        "angerona: set hook.angerona.event and hook.angerona.command in your global git config"
    );
    save_emails();
    0
}
