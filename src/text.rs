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

// A tab or a printable ASCII character, as a byte or a UTF-16 unit.
fn readable(u: u16) -> bool {
    u == 0x09 || (0x20..=0x7e).contains(&u)
}

// A file's content: text in full, or a binary file, one with a NUL in its first
// 8000 bytes as git reckons it, cut down to its runs of 6 or more readable
// characters, one per line. That keeps the text a program wrote into it, such
// as XMP or a PDF's properties, and drops most of its compressed bytes. Runs
// are read as bytes and as UTF-16 LE units at both alignments, which also
// covers UTF-16 BE. A binary file with a UTF-16 byte-order mark is UTF-16
// text, though: it is decoded and read in full.
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
    for run in bytes.split(|&b| !readable(u16::from(b))) {
        if run.len() >= 6 {
            runs.extend_from_slice(run);
            runs.push(b'\n');
        }
    }
    for start in [0, 1] {
        let units: Vec<u16> = bytes[start..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        for run in units.split(|&u| !readable(u)) {
            if run.len() >= 6 {
                // Each unit is readable, so it is one ASCII byte.
                runs.extend(run.iter().map(|&u| u as u8));
                runs.push(b'\n');
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        details::Details,
        matcher::{Kind, Matcher},
    };

    #[test]
    fn text_blob_is_read_whole() {
        let it = blob_item("a.txt", b"one\nzebra\n");
        assert!(matches!(&it.label, Label::Blob(p) if p == "a.txt"));
        assert_eq!(it.bytes, b"one\nzebra\n");
    }

    // Runs of 6 keep a short email or path. Every rule needs several specific
    // bytes in a row, so chance hits in compressed bytes stay negligible.
    #[test]
    fn binary_blob_keeps_readable_runs_of_six_or_more() {
        let mut b = b"\x00\x01abcde\x00".to_vec();
        b.extend(b"Author: zebra example inc\x00zebra!\x00");
        let it = blob_item("img.png", &b);
        assert!(matches!(&it.label, Label::Plain(l) if l == "binary img.png"));
        assert_eq!(it.bytes, b"Author: zebra example inc\nzebra!\n");
    }

    fn matcher() -> Matcher {
        Matcher::new(&Details {
            emails: vec![("me@home.example".into(), "test".into())],
            home: Some("/Users/jdoe".into()),
            raw_login: Some("jdoe".into()),
            login: Some("jdoe".into()),
            hosts: vec![],
            weak: vec![],
        })
    }

    fn kinds(bytes: &[u8]) -> Vec<Kind> {
        let it = blob_item("b.bin", bytes);
        let text = String::from_utf8_lossy(&it.bytes);
        matcher().find(&text).into_iter().map(|h| h.kind).collect()
    }

    #[test]
    fn a_short_email_between_nuls_is_found() {
        assert_eq!(kinds(b"\x00\x01me@home.example\x00"), vec![Kind::Email]);
    }

    #[test]
    fn utf16_without_a_byte_order_mark_is_found_at_either_alignment() {
        let path: Vec<u8> = "/Users/jdoe/x"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        for start in [&b"\x00\x01"[..], &b"\x00\x01\x02"[..]] {
            let b = [start, &path, &b"\x00\x00\x9c"[..]].concat();
            assert_eq!(kinds(&b), vec![Kind::HomePath], "{start:?}");
        }
    }

    #[test]
    fn utf16_with_a_byte_order_mark_is_text() {
        let units: Vec<u16> = "one\nzebra\n".encode_utf16().collect();
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
            assert_eq!(it.bytes, b"one\nzebra\n");
        }
    }

    #[test]
    fn a_byte_order_mark_without_a_nul_is_still_text() {
        let it = blob_item("t.txt", b"\xff\xfezebra\n");
        assert!(matches!(&it.label, Label::Blob(p) if p == "t.txt"));
        assert_eq!(it.bytes, b"\xff\xfezebra\n");
    }

    #[test]
    fn nul_after_8000_bytes_is_still_text() {
        let mut b = vec![b'a'; 8000];
        b.extend(b"\0zebra");
        assert!(matches!(blob_item("t", &b).label, Label::Blob(_)));
    }
}
