#[allow(dead_code)]
mod details;
mod doctor;
#[allow(dead_code)]
mod push;
#[allow(dead_code)]
mod repo;
#[allow(dead_code)]
mod text;

use std::{env, process::exit};

fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    match args.first().and_then(|a| a.to_str()) {
        Some("doctor") => exit(doctor::run()),
        _ => {
            eprintln!("usage: angerona check-push REMOTE URL | scan | doctor | setup");
            exit(2)
        }
    }
}
