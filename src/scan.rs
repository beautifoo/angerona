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
}

pub struct Item {
    pub label: Label,
    pub bytes: Vec<u8>,
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
            label: Label::Lines(""),
            bytes: b"acme\xffwidgets\n".to_vec(),
        };
        assert_eq!(
            hits(&m(&["Acme Widgets"]), &[item]),
            vec!["line 1: acme\u{FFFD}widgets"]
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
                ]
            ),
            vec!["line 1: Acme ⏎ Widgets", "message line 3: globex"]
        );
    }
}
