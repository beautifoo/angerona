use crate::repo;
use std::path::Path;

// What git keeps of a commit message written in an editor: nothing from the
// scissors line on, and no comment line. Comment lines turn into empty lines,
// so the line numbers stay those of the file.
pub fn message(text: &[u8], comment: &str) -> Vec<u8> {
    let dashes = "-".repeat(24);
    let scissors = format!("{comment} {dashes} >8 {dashes}");
    let mut out = Vec::new();
    for line in text.split_inclusive(|&b| b == b'\n') {
        if line.strip_suffix(b"\n").unwrap_or(line) == scissors.as_bytes() {
            break;
        }
        if !line.starts_with(comment.as_bytes()) {
            out.extend_from_slice(line);
        } else if line.ends_with(b"\n") {
            out.push(b'\n');
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
    fn blanks_comments_and_cuts_at_scissors() {
        assert_eq!(
            message(
                b"a\n# c\nglobex\n# ------------------------ >8 ------------------------\ndiff\n",
                "#"
            ),
            b"a\n\nglobex\n"
        );
        assert_eq!(
            message(
                b"a\n// c\n// ------------------------ >8 ------------------------\nx\n",
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
                "#"
            ),
            b"a\nx ------------------------ >8 ------------------------\nglobex\n"
        );
    }
}
