mod list;
mod msg;
mod push;
mod scan;
mod staged;

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
    panic::set_hook(Box::new(|info| {
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
        exit(1)
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
        ("check-push", Some(_)) => {
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
                    .and_then(|repo| push::items(&repo, &refs, &[]))
                    .unwrap_or_else(|e| fail(&format!("git failed reading the push ({e})"), 1)),
                1,
            )
        }
        _ => {
            eprintln!("usage: angerona check | check-msg FILE | check-staged | check-push REMOTE");
            exit(2)
        }
    }
}
