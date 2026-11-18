use std::{
    env,
    fs::OpenOptions,
    io::{BufRead, BufReader, Write},
};

// One answer to a yes/no question; None means it was neither, so ask again.
fn parse(answer: &str, default_yes: bool) -> Option<bool> {
    match answer.trim().to_lowercase().as_str() {
        "" => Some(default_yes),
        "y" | "yes" => Some(true),
        "n" | "no" => Some(false),
        _ => None,
    }
}

// Asks on the terminal, never on stdin: a hook's stdin holds the refs. None
// when there is no terminal to ask on.
pub fn ask(question: &str, default_yes: bool) -> Option<bool> {
    match env::var("TERM") {
        Ok(term) if !term.is_empty() && term != "dumb" => {}
        _ => return None,
    }
    let mut tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok()?;
    let mut reader = BufReader::new(tty.try_clone().ok()?);
    loop {
        write!(tty, "angerona: {question} ").ok()?;
        let mut line = String::new();
        // EOF means nobody is answering.
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        if let Some(answer) = parse(&line, default_yes) {
            return Some(answer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_read() {
        let got = [
            parse("", true),
            parse("", false),
            parse(" Y\n", false),
            parse("YES", false),
            parse("n", true),
            parse("No", true),
            parse("maybe", true),
        ];
        assert_eq!(
            got,
            [
                Some(true),
                Some(false),
                Some(true),
                Some(true),
                Some(false),
                Some(false),
                None
            ]
        );
    }
}
