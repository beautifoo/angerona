mod doctor;
mod json;
mod list;
mod msg;
mod push;
mod repo;
mod scan;
mod staged;
mod switch;

use scan::{Item, Label};
use std::{
    env, fs,
    io::{self, Read, Write},
    panic,
    path::Path,
    process::exit,
};

fn fail(reason: &str, code: i32) -> ! {
    eprintln!("angerona: {reason}, refusing to pass anything");
    exit(code)
}

// Loads the list and matches the items against it: exits 0 when none names a
// term, code after printing each hit when one does.
fn finish(items: &[Item], code: i32) -> ! {
    let m = list::load().unwrap_or_else(|why| {
        eprintln!("angerona: {why}");
        exit(code)
    });
    let found = scan::hits(&m, items);
    if found.is_empty() {
        exit(0);
    }
    eprintln!(
        "angerona: listed term(s) found; keep them out of what you publish:\n{}",
        found.join("\n")
    );
    exit(code)
}

fn read_stdin(code: i32) -> Vec<u8> {
    let mut bytes = Vec::new();
    if io::stdin().read_to_end(&mut bytes).is_err() {
        fail("can't read stdin", code);
    }
    bytes
}

fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let mode = args.first().and_then(|a| a.to_str()).unwrap_or("");
    // Claude Code blocks a call only on exit code 2; git refuses on any but 0.
    let code = if mode == "check-json" { 2 } else { 1 };
    panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let msg = if let Some(s) = payload.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown".to_string()
        };
        let _ = writeln!(
            io::stderr(),
            "angerona: internal error: {msg}, refusing to pass anything"
        );
        exit(code)
    }));

    match (mode, args.get(1).filter(|a| !a.is_empty())) {
        ("check", _) => {
            let bytes = read_stdin(code);
            finish(
                &[Item {
                    label: Label::Lines(""),
                    bytes,
                }],
                code,
            )
        }
        ("check-msg", Some(file)) => {
            let text = fs::read(file)
                .unwrap_or_else(|_| fail(&format!("can't read {}", file.to_string_lossy()), code));
            // git sets GIT_EDITOR to ":" when it runs no editor, and then keeps
            // every line.
            let bytes = if env::var_os("GIT_EDITOR").is_some_and(|v| v == ":") {
                text
            } else {
                msg::message(&text, &msg::comment_string())
            };
            finish(
                &[Item {
                    label: Label::Lines("message "),
                    bytes,
                }],
                code,
            )
        }
        ("check-staged", _) => {
            let items = repo::open(Path::new("."))
                .and_then(|repo| staged::items(&repo))
                .unwrap_or_else(|e| fail(&format!("git failed reading the commit ({e})"), code));
            finish(&items, code)
        }
        ("check-push", Some(remote)) => {
            let lines = read_stdin(code);
            let refs: Vec<_> = String::from_utf8_lossy(&lines)
                .lines()
                .filter_map(push::parse_ref_line)
                .collect();
            let repo = repo::open(Path::new("."))
                .unwrap_or_else(|e| fail(&format!("git failed reading the push ({e})"), code));
            // A push of only deletions sends nothing: no need to ask.
            let held = if refs.is_empty() {
                Vec::new()
            } else {
                let url = args.get(2).filter(|a| !a.is_empty());
                let url = url.map(|u| u.to_string_lossy());
                push::held(&repo, &remote.to_string_lossy(), url.as_deref())
            };
            if repo.common_dir().join("info/grafts").exists()
                || env::var_os("GIT_GRAFT_FILE").is_some()
            {
                fail("this repo has grafts, which gix ignores", code);
            }
            let items = push::items(&repo, &refs, &held)
                .unwrap_or_else(|e| fail(&format!("git failed reading the push ({e})"), code));
            finish(&items, code)
        }
        ("check-json", _) => {
            let bytes = read_stdin(code);
            let call: serde_json::Value = serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| fail("can't read the tool call as JSON", code));
            let input = call
                .get("tool_input")
                .filter(|v| !v.is_null())
                .unwrap_or_else(|| fail("the tool call has no tool_input", code));
            // A session's calls follow its project's git: where git skips the
            // push hook, for a repo that may name the terms, they pass, but a
            // GitHub call only to the repo's own owners. The project directory
            // is fixed when the session starts, unlike its cwd.
            let owners = env::var_os("CLAUDE_PROJECT_DIR")
                .filter(|d| !d.is_empty())
                .and_then(|d| json::work_owners(Path::new(&d)));
            let tool = call["tool_name"].as_str().unwrap_or_default();
            if owners.is_some_and(|o| json::passes(tool, input, &o)) {
                exit(0)
            }
            let cwd = call["cwd"].as_str().unwrap_or(".");
            finish(&json::items(input, Path::new(cwd)), code)
        }
        ("doctor", _) => exit(doctor::run()),
        _ => {
            eprintln!(
                "usage: angerona check | check-msg FILE | check-staged | check-push REMOTE | check-json | doctor"
            );
            exit(2)
        }
    }
}
