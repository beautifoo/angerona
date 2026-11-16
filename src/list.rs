use aho_corasick::AhoCorasick;
use std::{env, fs, path::PathBuf};

#[derive(Debug)]
pub enum ListError {
    NotATerm(usize),
    Empty,
}

// The terms, found as written, in any case, even inside a longer word.
pub struct Matcher {
    terms: Vec<String>,
    lower: AhoCorasick,
}

impl Matcher {
    pub fn new(terms: Vec<String>) -> Matcher {
        let lower = AhoCorasick::new(terms.iter().map(|t| t.to_lowercase()))
            .expect("the term list made a bad pattern");
        Matcher { terms, lower }
    }

    // Each hit in text as (line, term as listed): every term found, overlaps
    // included, by position. Lowercasing keeps every line break, so lines are
    // counted in the lowercased text.
    pub fn find(&self, text: &str) -> Vec<(usize, &str)> {
        let text = text.to_lowercase();
        let mut found: Vec<_> = self
            .lower
            .find_overlapping_iter(&text)
            .map(|m| (m.start(), m.pattern().as_usize()))
            .collect();
        found.sort();
        let (mut line, mut at) = (1, 0);
        found
            .into_iter()
            .map(|(start, term)| {
                line += text[at..start].matches('\n').count();
                at = start;
                (line, self.terms[term].as_str())
            })
            .collect()
    }
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

// One term per line, as written but for the whitespace around it. A line that
// would merge terms or names nothing refuses the whole list, by its number.
pub fn parse(text: &str) -> Result<Vec<String>, ListError> {
    let mut terms = Vec::new();
    for (i, line) in text.split('\n').enumerate() {
        // A list saved with CRLF line ends: the CR is not part of the term.
        let line = line.strip_suffix('\r').unwrap_or(line);
        let term = line.trim();
        if term.is_empty() || term.starts_with('#') {
            continue;
        }
        // A tab means two terms on one line, a CR left over means CR-only
        // line ends, and a # after whitespace a comment on the line of a term:
        // each would merge terms into one.
        let comment = line
            .match_indices('#')
            .any(|(at, _)| line[..at].ends_with(char::is_whitespace));
        if line.contains(['\t', '\r']) || comment || !term.contains(char::is_alphanumeric) {
            return Err(ListError::NotATerm(i + 1));
        }
        terms.push(term.to_string());
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
    Ok(Matcher::new(terms))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_terms_skipping_comments() {
        assert_eq!(
            parse("# c\n\n  # indented\nAcme Widgets\n\"Initech\"\n  globex.example \r\nZürich\n")
                .unwrap(),
            vec!["Acme Widgets", "\"Initech\"", "globex.example", "Zürich"]
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
    fn finds_every_term_as_listed_with_its_line() {
        let m = Matcher::new(vec!["acme".into(), "Acme Widgets".into(), "ZÜRICH".into()]);
        assert_eq!(
            m.find("x\nan ACME WIDGETS, acme-widgets\nzürich"),
            vec![(2, "acme"), (2, "Acme Widgets"), (2, "acme"), (3, "ZÜRICH")]
        );
    }
}
