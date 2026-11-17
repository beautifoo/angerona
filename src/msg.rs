use crate::repo;
use regex::bytes::Regex;
use std::path::Path;

// What git keeps of a commit message: with an editor, nothing after the
// scissors line and no comment line. Comment lines turn into empty lines, so
// the line numbers stay those of the file.
pub fn message(text: &[u8], editor_used: bool, comment: &str) -> Vec<u8> {
    if !editor_used {
        return text.to_vec();
    }
    let scissors = Regex::new(&format!(
        "(?m)^{} -{{24}} >8 -{{24}}$",
        regex::escape(comment)
    ))
    .unwrap();
    let kept = &text[..scissors.find(text).map_or(text.len(), |m| m.start())];
    let mut out = Vec::with_capacity(kept.len());
    for line in kept.split_inclusive(|&b| b == b'\n') {
        if line.starts_with(comment.as_bytes()) {
            out.extend(line.ends_with(b"\n").then_some(b'\n'));
        } else {
            out.extend_from_slice(line);
        }
    }
    out
}

// The comment string git uses: core.commentString, else core.commentChar.
// Git itself rejects an empty one and picks a character for auto, so both
// fall back to "#" rather than blank every line.
pub fn comment_string() -> String {
    let Ok(repo) = repo::open(Path::new(".")) else {
        return "#".into();
    };
    let config = repo.config_snapshot();
    config
        .string("core.commentString")
        .or_else(|| config.string("core.commentChar"))
        .map(|v| v.to_string())
        .filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("auto"))
        .unwrap_or_else(|| "#".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_comment_lines_when_no_editor_ran() {
        assert_eq!(
            message(b"subject\n\n# heading kept\n", false, "#"),
            b"subject\n\n# heading kept\n"
        );
    }

    #[test]
    fn blanks_comments_and_cuts_at_scissors_when_an_editor_ran() {
        assert_eq!(
            message(
                b"a\n# c\nglobex\n# ------------------------ >8 ------------------------\ndiff\n",
                true,
                "#"
            ),
            b"a\n\nglobex\n"
        );
        assert_eq!(
            message(
                b"a\n// c\n// ------------------------ >8 ------------------------\nx\n",
                true,
                "//"
            ),
            b"a\n\n"
        );
    }

    #[test]
    fn cuts_only_at_the_scissors_line_of_the_comment_string() {
        assert_eq!(
            message(
                b"a\nx ------------------------ >8 ------------------------\nglobex\n",
                true,
                "#"
            ),
            b"a\nx ------------------------ >8 ------------------------\nglobex\n"
        );
    }
}
