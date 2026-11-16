mod doctor;
mod json;
mod list;
mod msg;
mod push;
mod scan;
mod staged;
mod switch;

use scan::{Item, Label, fail, finish};
use std::{
    env, fs,
    io::{self, Read, Write},
    panic,
    path::Path,
    process::exit,
};

fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let mode = args.first().and_then(|a| a.to_str()).unwrap_or("");
    let code = if mode == "check-json" { 2 } else { 1 };
    panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let msg = match (
            payload.downcast_ref::<&str>(),
            payload.downcast_ref::<String>(),
        ) {
            (Some(s), _) => s.to_string(),
            (_, Some(s)) => s.clone(),
            _ => "unknown".to_string(),
        };
        let _ = writeln!(
            io::stderr(),
            "angerona: internal error: {msg}, refusing to pass anything"
        );
        exit(code)
    }));

    match (mode, args.get(1).filter(|a| !a.is_empty())) {
        ("check", _) => {
            let mut bytes = Vec::new();
            io::stdin()
                .read_to_end(&mut bytes)
                .unwrap_or_else(|_| fail("can't read stdin", 1));
            finish(
                &[Item {
                    label: Label::Lines(""),
                    bytes,
                }],
                1,
            )
        }
        ("check-msg", Some(file)) => {
            let text = fs::read(file)
                .unwrap_or_else(|_| fail(&format!("can't read {}", file.to_string_lossy()), 1));
            let editor_used = env::var_os("GIT_EDITOR").is_none_or(|v| v != ":");
            finish(
                &[Item {
                    label: Label::Lines("message "),
                    bytes: msg::message(&text, editor_used, &msg::comment_string()),
                }],
                1,
            )
        }
        ("check-staged", _) => finish(
            &staged::open(Path::new("."))
                .and_then(|repo| staged::items(&repo))
                .unwrap_or_else(|e| fail(&format!("git failed reading the commit ({e})"), 1)),
            1,
        ),
        ("check-push", Some(remote)) => {
            let mut lines = Vec::new();
            io::stdin()
                .read_to_end(&mut lines)
                .unwrap_or_else(|_| fail("can't read stdin", 1));
            let refs: Vec<_> = String::from_utf8_lossy(&lines)
                .lines()
                .filter_map(push::parse_ref_line)
                .collect();
            finish(
                &staged::open(Path::new("."))
                    .and_then(|repo| {
                        // A push of only deletions sends nothing: no need to ask.
                        let held = if refs.is_empty() {
                            Vec::new()
                        } else {
                            let url = args.get(2).filter(|a| !a.is_empty());
                            let url = url.map(|u| u.to_string_lossy());
                            push::held(&repo, &remote.to_string_lossy(), url.as_deref())
                        };
                        push::items(&repo, &refs, &held)
                    })
                    .unwrap_or_else(|e| fail(&format!("git failed reading the push ({e})"), 1)),
                1,
            )
        }
        ("check-json", _) => {
            let mut bytes = Vec::new();
            io::stdin()
                .read_to_end(&mut bytes)
                .unwrap_or_else(|_| fail("can't read stdin", 2));
            let call: serde_json::Value = serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| fail("can't read the tool call as JSON", 2));
            let input = call
                .get("tool_input")
                .filter(|v| !v.is_null())
                .unwrap_or_else(|| fail("the tool call has no tool_input", 2));
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
            finish(&json::items(input, Path::new(cwd)), 2)
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
