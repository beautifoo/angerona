use crate::repo;
use std::path::Path;

// What git keeps of a commit message written in an editor: no comment line,
// and with commit -v nothing from the scissors line on. The hook can't see -v,
// and without it git keeps what follows a scissors line, so the message is cut
// only where the first line past the comments, which git translates, is a diff,
// or there is none. Comment lines turn into empty lines, so the line numbers
// stay those of the file.
pub fn message(text: &[u8], comment: &str) -> Vec<u8> {
    let dashes = "-".repeat(24);
    let scissors = format!("{comment} {dashes} >8 {dashes}");
    let lines: Vec<_> = text.split_inclusive(|&b| b == b'\n').collect();
    let mut out = Vec::new();
    for (i, &line) in lines.iter().enumerate() {
        if line.strip_suffix(b"\n").unwrap_or(line) == scissors.as_bytes() {
            let next = lines[i + 1..]
                .iter()
                .find(|l| !l.starts_with(comment.as_bytes()));
            if next.is_none_or(|l| l.starts_with(b"diff --git ")) {
                break;
            }
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
                b"a\n# c\nglobex\n# ------------------------ >8 ------------------------\ndiff --git a/x b/x\n",
                "#"
            ),
            b"a\n\nglobex\n"
        );
        assert_eq!(
            message(
                b"a\n// c\n// ------------------------ >8 ------------------------\ndiff --git a/x b/x\n",
                "//"
            ),
            b"a\n\n"
        );
    }

    #[test]
    fn cuts_past_git_notes_in_any_language_or_where_nothing_follows() {
        assert_eq!(
            message(
                b"a\n# ------------------------ >8 ------------------------\n# \xc3\x84ndern oder entfernen Sie nicht die obige Zeile.\n# Alles unterhalb von ihr wird ignoriert.\ndiff --git a/x b/x\n-globex\n",
                "#"
            ),
            b"a\n"
        );
        assert_eq!(
            message(
                b"a\n# ------------------------ >8 ------------------------\n# notes\n",
                "#"
            ),
            b"a\n"
        );
    }

    #[test]
    fn reads_past_a_scissors_line_no_diff_follows() {
        assert_eq!(
            message(
                b"a\n# ------------------------ >8 ------------------------\nglobex\n",
                "#"
            ),
            b"a\n\nglobex\n"
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
