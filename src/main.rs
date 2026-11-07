mod list;
mod scan;

use scan::{Item, Label, fail, finish};
use std::{
    env,
    io::{self, Read, Write},
    panic,
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
        _ => {
            eprintln!("usage: angerona check");
            exit(2)
        }
    }
}
