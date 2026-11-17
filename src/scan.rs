use crate::list::Matcher;
use std::collections::HashSet;

// What an item is, which says where a hit in it is.
pub enum Label {
    // Lines of text, each hit at "<prefix>line N".
    Lines(&'static str),
    // A file's content, each hit at "<path>:N".
    Blob(String),
    // A name, each hit at "<what> <name>".
    Name(&'static str, String),
    // One thing, each hit at the label itself.
    Plain(String),
}

pub struct Item {
    pub label: Label,
    pub bytes: Vec<u8>,
}

// A file's content: text in full, or a binary file, one with a NUL in its first
// 8000 bytes as git reckons it, cut down to its runs of 16 or more readable
// characters, one per line. That keeps the text a program wrote into it, such
// as XMP or a PDF's properties, and drops its compressed bytes, where a short
// term turns up by chance. A binary file with a UTF-16 byte-order mark is
// UTF-16 text, though, which has NULs but no run of readable bytes: it is
// decoded and read in full.
pub fn blob_item(path: &str, bytes: &[u8]) -> Item {
    if !bytes[..bytes.len().min(8000)].contains(&0) {
        return Item {
            label: Label::Blob(path.into()),
            bytes: bytes.to_vec(),
        };
    }
    let utf16: Option<Vec<u16>> = match bytes {
        [0xff, 0xfe, rest @ ..] => Some(
            rest.chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect(),
        ),
        [0xfe, 0xff, rest @ ..] => Some(
            rest.chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect(),
        ),
        _ => None,
    };
    if let Some(units) = utf16 {
        return Item {
            label: Label::Blob(path.into()),
            bytes: String::from_utf16_lossy(&units).into_bytes(),
        };
    }
    let mut runs = Vec::new();
    for run in bytes.split(|b| !matches!(b, b'\t' | 0x20..=0x7e)) {
        if run.len() >= 16 {
            runs.extend_from_slice(run);
            runs.push(b'\n');
        }
    }
    Item {
        label: Label::Plain(format!("binary {path}")),
        bytes: runs,
    }
}

// A name, such as a file's, a ref's or a key's, read as itself.
pub fn name_item(what: &'static str, name: String) -> Item {
    let bytes = name.clone().into_bytes();
    Item {
        label: Label::Name(what, name),
        bytes,
    }
}

// Each hit as "<where>: <term>", once, in item order then by position.
pub fn hits(m: &Matcher, items: &[Item]) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for item in items {
        for (line, term) in m.find(&String::from_utf8_lossy(&item.bytes)) {
            let place = match &item.label {
                Label::Lines(prefix) => format!("{prefix}line {line}"),
                Label::Blob(path) => format!("{path}:{line}"),
                Label::Name(what, name) => format!("{what} {name}"),
                Label::Plain(place) => place.clone(),
            };
            let said = format!("{place}: {term}");
            if seen.insert(said.clone()) {
                out.push(said);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(t: &[&str]) -> Matcher {
        Matcher::new(t.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn matches_around_invalid_utf8() {
        let item = Item {
            label: Label::Blob("a".into()),
            bytes: b"caf\xe9\nglobex\xff\n".to_vec(),
        };
        assert_eq!(hits(&m(&["globex"]), &[item]), vec!["a:2: globex"]);
    }

    #[test]
    fn formats_each_label() {
        let mm = m(&["globex", "Acme Widgets"]);
        let it = |label, s: &str| Item {
            label,
            bytes: s.as_bytes().to_vec(),
        };
        assert_eq!(
            hits(
                &mm,
                &[
                    it(Label::Lines(""), "we sell ACME WIDGETS here"),
                    it(Label::Lines("message "), "fix\n\nfor the globex team\n"),
                    it(
                        Label::Blob("a.txt".into()),
                        "one\ntwo\nthree globex globex\n"
                    ),
                    it(Label::Name("file", "globex.txt".into()), "globex.txt"),
                    it(Label::Plain("identity".into()), "T <me@globex.example>"),
                ]
            ),
            vec![
                "line 1: Acme Widgets",
                "message line 3: globex",
                "a.txt:3: globex",
                "file globex.txt: globex",
                "identity: globex"
            ]
        );
    }

    #[test]
    fn text_blob_is_read_whole() {
        let it = blob_item("a.txt", b"one\nglobex\n");
        assert!(matches!(&it.label, Label::Blob(p) if p == "a.txt"));
        assert_eq!(it.bytes, b"one\nglobex\n");
    }

    #[test]
    fn binary_blob_keeps_only_long_readable_runs() {
        let mut b = b"\x00\x01ab\x00".to_vec();
        b.extend(b"Author: globex example inc\x00short globex\x00");
        let it = blob_item("img.png", &b);
        assert!(matches!(&it.label, Label::Plain(l) if l == "binary img.png"));
        assert_eq!(it.bytes, b"Author: globex example inc\n");
    }

    #[test]
    fn utf16_with_a_byte_order_mark_is_text() {
        let units: Vec<u16> = "one\nglobex\n".encode_utf16().collect();
        let le = [
            &[0xff, 0xfe][..],
            &units
                .iter()
                .flat_map(|u| u.to_le_bytes())
                .collect::<Vec<_>>(),
        ]
        .concat();
        let be = [
            &[0xfe, 0xff][..],
            &units
                .iter()
                .flat_map(|u| u.to_be_bytes())
                .collect::<Vec<_>>(),
        ]
        .concat();
        for b in [le, be] {
            let it = blob_item("t.txt", &b);
            assert!(matches!(&it.label, Label::Blob(p) if p == "t.txt"));
            assert_eq!(it.bytes, b"one\nglobex\n");
        }
    }

    #[test]
    fn a_byte_order_mark_without_a_nul_is_still_text() {
        let it = blob_item("t.txt", b"\xff\xfeglobex\n");
        assert!(matches!(&it.label, Label::Blob(p) if p == "t.txt"));
        assert_eq!(it.bytes, b"\xff\xfeglobex\n");
    }

    #[test]
    fn nul_after_8000_bytes_is_still_text() {
        let mut b = vec![b'a'; 8000];
        b.extend(b"\0globex");
        assert!(matches!(blob_item("t", &b).label, Label::Blob(_)));
    }
}
