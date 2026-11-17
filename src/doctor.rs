use crate::{
    list, repo,
    switch::{self, By, Switch},
};
use regex::Regex;
use serde_json::{Value, json};
use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

const PROBE: &str = "mcp__x__y";
const NEW_GIT: &str = "install git 2.54 or later, e.g. brew install git";

fn executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

// What git prints on stdout, or nothing when it can't run.
fn git(args: &[&str]) -> String {
    Command::new("git")
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

// Checks that git runs the guard here, that the list is in place and that
// Claude Code runs check-json; prints each problem with the command that
// fixes it.
pub fn run() -> i32 {
    let mut problems = list_problem();
    let mut notes = String::new();
    // A git that can't run the hooks would make their fixes mislead.
    if check_gits(&mut problems, &mut notes) {
        problems += &hook_problems();
    }
    problems += &claude_problem();

    if !notes.is_empty() {
        print!("angerona: note: {notes}");
    }
    if problems.is_empty() {
        println!("angerona: the guard is in place");
        0
    } else {
        print!("angerona: {problems}");
        1
    }
}

fn list_problem() -> String {
    let terms = list::path();
    let t = terms.display();
    match list::load() {
        Ok(_) if fs::metadata(&terms).is_ok_and(|m| m.permissions().mode() & 0o044 != 0) => {
            format!("{t} is readable by others; fix: chmod 600 {t}\n")
        }
        Ok(_) => String::new(),
        Err(e) => format!("{e}; write it (see README.md) or restore it, then: chmod 600 {t}\n"),
    }
}

// A git before 2.54, or one that can't run, skips config hooks without a
// word. The first git on PATH is the one that runs them here, so it adds to
// problems; another adds only to notes, since an app may still commit with
// it. Says whether the first one runs them.
fn check_gits(problems: &mut String, notes: &mut String) -> bool {
    let mut gits: Vec<PathBuf> = Vec::new();
    let path = env::var_os("PATH").unwrap_or_default();
    for dir in env::split_paths(&path).filter(|d| !d.as_os_str().is_empty()) {
        let g = dir.join("git");
        if executable(&g) && !gits.contains(&g) {
            gits.push(g);
        }
    }
    let Some(first) = gits.first() else {
        *problems += &format!("no git on PATH; fix: {NEW_GIT}\n");
        return false;
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
    let mut first_runs_hooks = true;
    for (g, version) in bad {
        let fix = match good {
            None => NEW_GIT.to_string(),
            Some(good) if g == first => format!("put {} before {} on PATH", dir(good), dir(g)),
            Some(good) => format!("point any app that uses it at {}", good.display()),
        };
        let msg = format!(
            "{} ({version}) skips config hooks, so a commit or push made with it skips the guard; fix: {fix}\n",
            g.display()
        );
        if g == first {
            first_runs_hooks = false;
            *problems += &msg;
        } else {
            *notes += &msg;
        }
    }
    first_runs_hooks
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

// Whether git runs each guard hook here, and if not, why and how to fix it.
fn hook_problems() -> String {
    // Fold a "..", so the fix reads the same however the binary was run.
    let me = env::current_exe().map_or("angerona".into(), |p| {
        repo::fold_dotdot(&p).display().to_string()
    });
    let home = env::var("HOME").ok();
    let mut problems = String::new();
    for (name, event, mode) in [
        ("msg", "commit-msg", "check-msg"),
        ("staged", "pre-commit", "check-staged"),
        ("push", "pre-push", "check-push"),
    ] {
        let hook = format!("angerona-{name}");
        // git's own answer: whether it runs the hook here, skips it, or has
        // none set up. git lists a switched-off hook only once it is set up.
        let list = git(&["hook", "list", event]);
        let git_runs = list
            .lines()
            .find_map(|l| match l.split('\t').collect::<Vec<_>>()[..] {
                [n] if n == hook => Some(true),
                ["disabled" | "event-disabled", n, ..] if n == hook => Some(false),
                _ => None,
            });
        // Whether the command it runs is angerona, by a path that exists,
        // in this mode.
        let cmd = git(&["config", "--get", &format!("hook.{hook}.command")]);
        let runs = match cmd.trim_end_matches('\n').split_once(' ') {
            Some((path, args)) => {
                let path = match (path.strip_prefix('~'), &home) {
                    (Some(rest), Some(home)) => format!("{home}{rest}"),
                    _ => path.to_string(),
                };
                executable(Path::new(&path)) && args == mode
            }
            None => false,
        };
        let missing = format!(
            "no {hook} hook for {event} here; fix:\ngit config set --global hook.{hook}.event {event}\ngit config set --global hook.{hook}.command '{me} {mode}'\n"
        );
        // Switched off on purpose, where the terms belong, or by mistake:
        // either way the guard doesn't run here, and setting the hook again
        // won't help. check-json reads the same switch, less the system
        // config, so where it and git disagree, that is a problem too.
        problems += &match (switch::read(Path::new("."), &hook, true), git_runs) {
            (Switch::Unclear(why), _) => {
                format!("{hook}: can't tell whether it is switched off here, {why}\n")
            }
            (_, None) => missing,
            (Switch::On, Some(true)) if runs => String::new(),
            (Switch::On, Some(true)) => missing,
            (Switch::On, Some(false)) => format!(
                "{hook}: git skips it here, but angerona reads it as on, from config it can't read; find it with: git config --show-origin --get-regexp '^hook\\.'\n"
            ),
            (Switch::Off(by), Some(true)) => format!(
                "{hook}: git runs it here, but angerona reads it as switched off, by {by}\n"
            ),
            (Switch::Off(By::Repo(scope, file)), Some(false)) => format!(
                "{hook} is switched off by this repo's own config, {file}, which an agent can write; to switch it off on purpose, use an include in your global config (see README.md); if you didn't, fix: git config unset --{scope} hook.{hook}.enabled\n"
            ),
            (Switch::Off(By::File(file)), Some(false)) => format!(
                "{hook} is switched off here, by {file}; if this repo must not name the terms, fix: git config unset --file {file} hook.{hook}.enabled\n"
            ),
            (Switch::Off(By::Env), Some(false)) => format!(
                "{hook} is switched off here, by command line:; if this repo must not name the terms, fix: unset GIT_CONFIG_COUNT\n"
            ),
        };
    }
    problems
}

// A PreToolUse entry whose matcher covers MCP tools, and whose hook runs
// check-json and blocks when it fails, in the user's settings or the
// managed ones, and not switched off: by disableAllHooks, or, in the
// managed ones, by allowManagedHooksOnly, which drops the user's hooks.
// Only checked where Claude Code is set up.
fn claude_problem() -> String {
    let Some(home) = env::var("HOME")
        .ok()
        .filter(|h| Path::new(h).join(".claude").is_dir())
    else {
        return String::new();
    };
    let managed = if cfg!(target_os = "macos") {
        "/Library/Application Support/ClaudeCode/managed-settings.json"
    } else {
        "/etc/claude-code/managed-settings.json"
    };
    let user = read_settings(&format!("{home}/.claude/settings.json"));
    let managed = read_settings(managed);
    let switched_off = user["disableAllHooks"] == true || managed["disableAllHooks"] == true;
    let hooked = if managed["allowManagedHooksOnly"] == true {
        has_guard_hook(&managed)
    } else {
        has_guard_hook(&user) || has_guard_hook(&managed)
    };
    if switched_off || !hooked {
        "no Claude Code hook runs angerona check-json on every MCP call; fix: add the hook in README.md to ~/.claude/settings.json\n".into()
    } else {
        String::new()
    }
}

// A settings file, or null when it is missing or not JSON.
fn read_settings(path: &str) -> Value {
    let Ok(bytes) = fs::read(path) else {
        return Value::Null;
    };
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

fn has_guard_hook(settings: &Value) -> bool {
    let Some(entries) = settings["hooks"]["PreToolUse"].as_array() else {
        return false;
    };
    for entry in entries {
        let Some(hooks) = entry["hooks"].as_array() else {
            continue;
        };
        if covers(entry["matcher"].as_str().unwrap_or("")) && hooks.iter().any(runs_check_json) {
            return true;
        }
    }
    false
}

// Whether a matcher covers the probe. A matcher of plain words is a list of
// exact tool names, which covers the probe only when one of them is its
// name; "" and "*" take every tool; any other is a regex.
fn covers(matcher: &str) -> bool {
    if matcher.is_empty() || matcher == "*" {
        return true;
    }
    let plain = matcher
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "_-, |".contains(c));
    if plain {
        matcher.split(['|', ',']).any(|name| name.trim() == PROBE)
    } else {
        Regex::new(matcher).is_ok_and(|re| re.is_match(PROBE))
    }
}

fn runs_check_json(hook: &Value) -> bool {
    let cmd = hook["command"].as_str().unwrap_or("");
    let runs = (cmd.ends_with("angerona") && hook["args"] == json!(["check-json"]))
        || cmd.contains("angerona check-json");
    runs && hook["onFailure"] == "block"
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
