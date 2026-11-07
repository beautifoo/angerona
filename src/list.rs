use crate::scan::Matcher;
use regex::Regex;
use std::{env, fs, path::PathBuf};

#[derive(Debug)]
pub enum ListError {
    NotATerm(usize),
    Empty,
}

// ${XDG_CONFIG_HOME:-$HOME/.config}/angerona/terms.txt. Only an absolute
// directory counts, since a relative one would read the list from the working
// directory, which under a git hook is the repo being scanned. A HOME that is
// not absolute counts as /, as it does in the shell.
pub fn path() -> PathBuf {
    let absolute = |var| {
        env::var_os(var)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    let base = absolute("XDG_CONFIG_HOME").unwrap_or_else(|| {
        absolute("HOME")
            .unwrap_or_else(|| "/".into())
            .join(".config")
    });
    base.join("angerona/terms.txt")
}

// One term per line, as its words. A line that would merge terms or names
// nothing refuses the whole list, by its number.
pub fn parse(text: &str) -> Result<Vec<Vec<String>>, ListError> {
    let words = Regex::new(r"[\p{L}\p{N}]+").unwrap();
    let mut terms = Vec::new();
    for (i, line) in text.split('\n').enumerate() {
        // A list saved with CRLF line ends: the CR is not part of the term.
        let line = line.strip_suffix('\r').unwrap_or(line);
        let rest = line.trim_start();
        if rest.is_empty() || rest.starts_with('#') {
            continue;
        }
        // A tab means two terms on one line, a CR left over means CR-only
        // line ends, and a # after a space a comment on the line of a term: each
        // would merge terms into one.
        let comment = line
            .match_indices('#')
            .any(|(at, _)| line[..at].ends_with(char::is_whitespace));
        let term: Vec<String> = words
            .find_iter(line)
            .map(|w| w.as_str().to_string())
            .collect();
        if line.contains(['\t', '\r']) || comment || term.is_empty() {
            return Err(ListError::NotATerm(i + 1));
        }
        terms.push(term);
    }
    if terms.is_empty() {
        Err(ListError::Empty)
    } else {
        Ok(terms)
    }
}

// The error is the whole reason, as the refusal prints it after "angerona: ".
pub fn load() -> Result<Matcher, String> {
    let path = path();
    let list = path.display();
    let refuse = |why: String| format!("{why}, refusing to pass anything");
    let text = fs::read_to_string(&path).map_err(|_| refuse(format!("can't read {list}")))?;
    let terms = parse(&text).map_err(|e| match e {
        ListError::NotATerm(n) => refuse(format!("line {n} of {list} is not a term")),
        ListError::Empty => refuse(format!("no term in {list}")),
    })?;
    Ok(Matcher::new(&terms))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_terms_skipping_comments() {
        assert_eq!(
            parse("# c\n\n  # indented\nAcme Widgets\n\"Initech\"\nglobex.example\r\n").unwrap(),
            vec![
                vec!["Acme", "Widgets"],
                vec!["Initech"],
                vec!["globex", "example"]
            ]
        );
    }

    #[test]
    fn refuses_bad_lines_by_number() {
        for (text, n) in [
            ("ok\na\tb\n", 2),
            ("x\ra\n", 1),
            ("acme # note\n", 1),
            ("---\n", 1),
        ] {
            assert!(
                matches!(parse(text), Err(ListError::NotATerm(m)) if m == n),
                "{text:?}"
            );
        }
        assert!(matches!(parse("# only\n\n"), Err(ListError::Empty)));
    }

    #[test]
    fn accepts_any_script() {
        assert_eq!(
            parse("Zürich\nМосква\n").unwrap(),
            vec![vec!["Zürich"], vec!["Москва"]]
        );
    }
}
