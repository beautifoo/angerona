use crate::list;
use regex::{Regex, RegexBuilder};
use std::{cmp::Reverse, collections::HashSet, process::exit};

pub struct Matcher(Regex);

impl Matcher {
    // Each term's words are joined by any run of other characters, or none.
    // The longer terms come first, so that of two overlapping terms the longer
    // is the one reported. Each gap compiles to some 35 KB, so the default 10 MB
    // limit would stop a list of 100 four-word terms, and the default 2 MB lazy
    // DFA cache would thrash on one (50 times slower); both are raised.
    pub fn new(terms: &[Vec<String>]) -> Matcher {
        let mut alts: Vec<_> = terms
            .iter()
            .map(|words| {
                let escaped: Vec<_> = words.iter().map(|w| regex::escape(w)).collect();
                (words.concat().len(), escaped.join(r"[^\p{L}\p{N}]*"))
            })
            .collect();
        alts.sort_by_key(|(len, _)| Reverse(*len));
        let alts: Vec<_> = alts.into_iter().map(|(_, alt)| alt).collect();
        Matcher(
            RegexBuilder::new(&format!("(?i)(?:{})", alts.join("|")))
                .size_limit(1 << 28)
                .dfa_size_limit(1 << 28)
                .build()
                .expect("the term list made a bad pattern"),
        )
    }

    pub fn find<'a>(&self, text: &'a str) -> Vec<(usize, &'a str)> {
        self.0
            .find_iter(text)
            .map(|m| (m.start(), m.as_str()))
            .collect()
    }
}

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
    let utf16 = match bytes {
        [0xff, 0xfe, rest @ ..] => Some((rest, u16::from_le_bytes as fn(_) -> _)),
        [0xfe, 0xff, rest @ ..] => Some((rest, u16::from_be_bytes as fn(_) -> _)),
        _ => None,
    };
    if let Some((rest, unit)) = utf16 {
        let units: Vec<_> = rest.chunks_exact(2).map(|c| unit([c[0], c[1]])).collect();
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

// Each hit as "<where>: <match>", once, in item order then by position.
pub fn hits(m: &Matcher, items: &[Item]) -> Vec<String> {
    let (mut out, mut seen) = (Vec::new(), HashSet::new());
    for item in items {
        let text = String::from_utf8_lossy(&item.bytes);
        let (mut line, mut at) = (1, 0);
        for (start, hit) in m.find(&text) {
            line += text[at..start].matches('\n').count();
            at = start;
            let place = match &item.label {
                Label::Lines(prefix) => format!("{prefix}line {line}"),
                Label::Blob(path) => format!("{path}:{line}"),
                Label::Name(what, name) => format!("{what} {name}"),
                Label::Plain(place) => place.clone(),
            };
            let said = format!(
                "{place}: {}",
                hit.replace("\r\n", " ⏎ ").replace('\n', " ⏎ ")
            );
            if seen.insert(said.clone()) {
                out.push(said);
            }
        }
    }
    out
}

// Loads the list and matches the items against it: exits 0 when none names a
// term, refuse_code after printing each hit when one does.
pub fn finish(items: &[Item], refuse_code: i32) -> ! {
    let m = list::load().unwrap_or_else(|why| {
        eprintln!("angerona: {why}");
        exit(refuse_code)
    });
    let found = hits(&m, items);
    if found.is_empty() {
        exit(0);
    }
    eprintln!(
        "angerona: listed term(s) found; keep them out of what you publish:\n{}",
        found.join("\n")
    );
    exit(refuse_code)
}

pub fn fail(reason: &str, code: i32) -> ! {
    eprintln!("angerona: {reason}, refusing to pass anything");
    exit(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::list;

    fn m(t: &[&str]) -> Matcher {
        Matcher::new(
            &t.iter()
                .map(|s| list::parse(s).unwrap().remove(0))
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn matches_inside_words_reporting_only_the_term() {
        assert_eq!(
            m(&["globex"]).find("the xglobexy plan"),
            vec![(5, "globex")]
        );
    }

    #[test]
    fn matches_any_separator_or_none() {
        for t in [
            "acme_widgets",
            "AcmeWidgets",
            "acme .. widgets",
            "Acme\nWidgets",
            "Acme.\n\nWidgets",
        ] {
            assert_eq!(m(&["Acme Widgets"]).find(t).len(), 1, "{t:?}");
        }
        assert!(
            m(&["Acme Widgets"])
                .find("acme gadgets and widgets")
                .is_empty()
        );
        assert!(m(&["qu+ux"]).find("quuux").is_empty());
    }

    #[test]
    fn folds_unicode_case() {
        assert_eq!(m(&["Zürich"]).find("ZÜRICH and zürich").len(), 2);
        assert!(m(&["Zürich"]).find("Z-rich or Zurich").is_empty());
    }

    #[test]
    fn matches_across_invalid_utf8() {
        let item = Item {
            label: Label::Blob("a".into()),
            bytes: b"acme\xffwidgets\n".to_vec(),
        };
        assert_eq!(
            hits(&m(&["Acme Widgets"]), &[item]),
            vec!["a:1: acme\u{FFFD}widgets"]
        );
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
                    it(Label::Lines(""), "we sell Acme\nWidgets here"),
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
                "line 1: Acme ⏎ Widgets",
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
