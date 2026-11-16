use crate::{
    list,
    switch::{self, Switch},
};
use regex::Regex;
use serde_json::{Value, json};
use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Component, Path, PathBuf},
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
    let (mut notes, mut problems, mut old) = (String::new(), String::new(), false);
    // Fold a "..", so the fix reads the same however the binary was run.
    let me = env::current_exe().map_or("angerona".into(), |p| {
        let mut clean = PathBuf::new();
        for part in p.components() {
            if part == Component::ParentDir {
                clean.pop();
            } else {
                clean.push(part);
            }
        }
        clean.display().to_string()
    });
    let home = env::var("HOME").ok();

    let terms = list::path();
    let t = terms.display();
    match list::load() {
        Ok(_) => {
            if fs::metadata(&terms).is_ok_and(|m| m.permissions().mode() & 0o044 != 0) {
                problems += &format!("{t} is readable by others; fix: chmod 600 {t}\n");
            }
        }
        Err(e) => {
            problems +=
                &format!("{e}; write it (see README.md) or restore it, then: chmod 600 {t}\n")
        }
    }

    // A git before 2.54, or one that can't run, skips config hooks without a
    // word. The first git on PATH is the one that runs them here; another is
    // only noted, since an app may still commit with it. The version is read
    // from stdout, past any warning, and the error only when there is none.
    let mut gits: Vec<PathBuf> = Vec::new();
    let path = env::var_os("PATH").unwrap_or_default();
    for dir in env::split_paths(&path).filter(|d| !d.as_os_str().is_empty()) {
        let g = dir.join("git");
        if executable(&g) && !gits.contains(&g) {
            gits.push(g);
        }
    }
    let first = gits.first();
    if first.is_none() {
        old = true;
        problems += &format!("no git on PATH; fix: {NEW_GIT}\n");
    }
    let (mut good, mut bad) = (None, Vec::new());
    for g in &gits {
        let out = Command::new(g).arg("--version").output();
        let line = |b: &[u8]| {
            String::from_utf8_lossy(b)
                .lines()
                .next()
                .unwrap_or("")
                .to_string()
        };
        let ver = out.map_or(String::new(), |o| match line(&o.stdout) {
            v if v.is_empty() => line(&o.stderr),
            v => v,
        });
        let n: Vec<&str> = ver.split([' ', '.']).collect();
        let num = |i: usize| {
            let digits: String = n
                .get(i)
                .map_or("", |s| s)
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            digits.parse::<u32>().unwrap_or(0)
        };
        if num(2) > 2 || num(2) == 2 && num(3) >= 54 {
            good = good.or(Some(g));
        } else {
            bad.push((g, ver));
        }
    }
    let dir = |g: &Path| g.parent().unwrap_or(g).display().to_string();
    for (g, ver) in bad {
        let fix = match good {
            None => NEW_GIT.to_string(),
            Some(good) if Some(g) == first => {
                format!("put {} before {} on PATH", dir(good), dir(g))
            }
            Some(good) => format!("point any app that uses it at {}", good.display()),
        };
        let msg = format!(
            "{} ({ver}) skips config hooks, so a commit or push made with it skips the guard; fix: {fix}\n",
            g.display()
        );
        if Some(g) == first {
            old = true;
            problems += &msg;
        } else {
            notes += &msg;
        }
    }

    for (name, event, mode) in [
        ("msg", "commit-msg", "check-msg"),
        ("staged", "pre-commit", "check-staged"),
        ("push", "pre-push", "check-push"),
    ] {
        // This git can't run them, so their fixes would mislead.
        if old {
            break;
        }
        let hook = format!("angerona-{name}");
        // Switched off on purpose, where the terms belong, or by mistake:
        // either way the guard doesn't run here, and setting the hook again
        // won't help. check-json reads the same switch.
        let off = match switch::read(Path::new("."), &hook, event) {
            Switch::On => None,
            Switch::Off {
                key,
                scope: Some(scope),
                by,
            } => Some(format!(
                "{hook} is switched off by this repo's own config, {by}, which an agent can write; to switch it off on purpose, use an include in your global config (see README.md); if you didn't, fix: git config unset --{scope} {key}\n"
            )),
            Switch::Off { key, by, .. } => Some(format!(
                "{hook} is switched off here, by {by}; if this repo must not name the terms, fix: git config unset --file {by} {key}\n"
            )),
            Switch::Unclear(why) => Some(format!(
                "{hook}: can't tell whether it is switched off here, {why}\n"
            )),
        };
        if let Some(off) = off {
            problems += &off;
            continue;
        }
        let list = git(&["hook", "list", event]);
        let cmd = git(&["config", "--get", &format!("hook.{hook}.command")]);
        let runs = cmd
            .trim_end_matches('\n')
            .split_once(' ')
            .is_some_and(|(path, args)| {
                let path = match (path.strip_prefix('~'), &home) {
                    (Some(rest), Some(home)) => format!("{home}{rest}"),
                    _ => path.to_string(),
                };
                executable(Path::new(&path)) && args == mode
            });
        if !(list.lines().any(|l| l == hook) && runs) {
            problems += &format!(
                "no {hook} hook for {event} here; fix:\ngit config set --global hook.{hook}.event {event}\ngit config set --global hook.{hook}.command '{me} {mode}'\n"
            );
        }
    }

    // A PreToolUse entry whose matcher covers MCP tools, and whose hook runs
    // check-json and blocks when it fails, in the user's settings or the
    // managed ones, and not switched off: by disableAllHooks, or, in the
    // managed ones, by allowManagedHooksOnly, which drops the user's hooks.
    if let Some(home) = home.filter(|h| Path::new(h).join(".claude").is_dir()) {
        let managed = if cfg!(target_os = "macos") {
            "/Library/Application Support/ClaudeCode/managed-settings.json"
        } else {
            "/etc/claude-code/managed-settings.json"
        };
        let [user, managed] = [format!("{home}/.claude/settings.json"), managed.into()].map(|p| {
            let read = |p: String| serde_json::from_slice::<Value>(&fs::read(p).ok()?).ok();
            read(p).unwrap_or(Value::Null)
        });
        // A matcher of plain words is a list of exact tool names, which covers
        // the probe only when one of them is its name; "" and "*" take every
        // tool; any other is a regex.
        let covers = |m: &str| match m {
            "" | "*" => true,
            _ if m
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-, |".contains(c)) =>
            {
                m.split(['|', ',']).any(|n| n.trim() == PROBE)
            }
            _ => Regex::new(m).is_ok_and(|re| re.is_match(PROBE)),
        };
        let sources = if managed["allowManagedHooksOnly"] == true {
            vec![&managed]
        } else {
            vec![&user, &managed]
        };
        let hooked = [&user, &managed]
            .iter()
            .all(|s| s["disableAllHooks"] != true)
            && sources.iter().any(|s| {
                let entries = s["hooks"]["PreToolUse"].as_array();
                entries.is_some_and(|es| {
                    es.iter().any(|e| {
                        covers(e["matcher"].as_str().unwrap_or(""))
                            && e["hooks"].as_array().is_some_and(|hs| {
                                hs.iter().any(|h| {
                                    let cmd = h["command"].as_str().unwrap_or("");
                                    let runs = cmd.ends_with("angerona")
                                        && h["args"] == json!(["check-json"])
                                        || cmd.contains("angerona check-json");
                                    runs && h["onFailure"] == "block"
                                })
                            })
                    })
                })
            });
        if !hooked {
            problems += "no Claude Code hook runs angerona check-json on every MCP call; fix: add the hook in README.md to ~/.claude/settings.json\n";
        }
    }

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
