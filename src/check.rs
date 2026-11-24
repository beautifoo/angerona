use crate::{
    added::commit_items,
    identity::{self, foreign},
    matcher::{Hit, Kind as HitKind, Matcher},
    push::{Outgoing, RefLine, read},
    text::{Item, Label, blob_item, name_item},
};
use gix::{
    ObjectId,
    objs::{CommitRefIter, Kind, TagRefIter},
};
use std::error::Error;

pub const IN_FILE: &str = "edit the file, then amend or rebase";
const IN_AUTHOR: &str = "git rebase -i <base> --exec 'git commit --amend --no-edit --reset-author'";
const IN_COMMITTER: &str = "with the right user.email set, git rebase -i <base> --exec 'git commit --amend --no-edit' (keeps the author)";
const IN_TAG: &str = "re-create the tag, with the right user.email set: git tag -d <name> && git tag -a <name> <commit>";
const IN_MESSAGE: &str = "git rebase -i, reword";
const IN_HEADER: &str =
    "make the commit again; for a merged signed tag, merge <tag>^{commit} instead";
pub const IN_REF: &str = "push under another name: git push <remote> HEAD:refs/heads/<name>";

// One personal detail, where it is, and how to take it out. `at`, `what` and
// `hint` are printed; `kind`, `address` and `ident` say the same as data: the
// address the detail is, if it is one, and the commit or tag header field it
// sits in, if it does.
pub struct Finding {
    pub at: String,
    pub what: String,
    pub hint: &'static str,
    pub kind: HitKind,
    pub address: Option<String>,
    pub ident: Option<(Kind, String, String)>,
}

impl Finding {
    // A hit, with the kind of detail and the text as found, so it can be
    // searched for.
    pub fn new(at: String, h: &Hit, hint: &'static str) -> Finding {
        let kind = match h.kind {
            HitKind::Email => "email",
            HitKind::HomePath => "home path",
            HitKind::Login => "login",
            HitKind::Host => "machine name",
        };
        Finding {
            at,
            what: format!("{kind} {}", h.text),
            hint,
            kind: h.kind,
            address: h.address.clone(),
            ident: None,
        }
    }
}

// Whether a finding is one of these lowercased addresses, whatever rule found
// it.
pub fn quotes_any(f: &Finding, addresses: &[String]) -> bool {
    f.address
        .as_ref()
        .is_some_and(|a| addresses.contains(&a.to_lowercase()))
}

// Each hit in an item, at the place its label names.
pub fn item_findings(m: &Matcher, object: &str, item: &Item, hint: &'static str) -> Vec<Finding> {
    let text = String::from_utf8_lossy(&item.bytes);
    let mut found = Vec::new();
    for h in m.find(&text) {
        let place = match &item.label {
            Label::Lines(prefix) => format!("{prefix}line {}", h.line),
            Label::Blob(path) => format!("{path}:{}", h.line),
            Label::Name(kind, name) => format!("{kind} {name}"),
            Label::Plain(label) => label.clone(),
        };
        found.push(Finding::new(format!("{object} {place}"), &h, hint));
    }
    found
}

// Who a commit or tag names, and what its message says. A trailer is part of
// the message, so it is found there, with the hint that fixes a message; the
// header identities are matched on their own, and the other headers, such as
// a merged tag's, together as one. Another person's email counts
// only where identity::foreign says. Anything in a tag is fixed by making
// the tag again; a commit's committer, by a rebase that keeps the author.
// The other headers count as one field, "header", so a scan folds an address
// there into the address's line.
fn object_findings(
    raw: &[u8],
    id: ObjectId,
    unpublished: bool,
    m: &Matcher,
    own: &[String],
) -> Vec<Finding> {
    let is_tag = raw.starts_with(b"object ");
    let kind = if is_tag { Kind::Tag } else { Kind::Commit };
    let hash = id.to_hex_with_len(7).to_string();
    let object = format!("{kind} {hash}");
    let ident_hint = |field: &str| {
        if is_tag {
            IN_TAG
        } else if field == "author" {
            IN_AUTHOR
        } else {
            IN_COMMITTER
        }
    };
    let in_field = |item: &Item, field: &str, hint| {
        let ident = Some((kind, hash.clone(), field.to_string()));
        item_findings(m, &object, item, hint)
            .into_iter()
            .map(move |f| Finding {
                ident: ident.clone(),
                ..f
            })
    };
    let o = identity::read(raw);
    let mut found = Vec::new();
    for i in &o.idents {
        if ["author", "committer", "tagger"].contains(&i.field.as_str()) {
            let item = Item {
                label: Label::Plain(i.field.clone()),
                bytes: i.raw.clone().into_bytes(),
            };
            found.extend(in_field(&item, &i.field, ident_hint(&i.field)));
        }
    }
    let header = Item {
        label: Label::Plain("header".to_string()),
        bytes: o.header.clone(),
    };
    let header_hint = if is_tag { IN_TAG } else { IN_HEADER };
    found.extend(in_field(&header, "header", header_hint));
    let message = Item {
        label: Label::Lines("message "),
        bytes: o.message.clone(),
    };
    let message_hint = if is_tag { IN_TAG } else { IN_MESSAGE };
    found.extend(item_findings(m, &object, &message, message_hint));
    for i in foreign(&o, unpublished, own) {
        found.push(Finding {
            at: format!("{object} {}", i.field),
            what: format!("email {}", i.email),
            hint: ident_hint(&i.field),
            kind: HitKind::Email,
            address: Some(i.email.clone()),
            ident: Some((kind, hash.clone(), i.field.clone())),
        });
    }
    found
}

// What a commit adds, who it names and what its message says. A parent that
// can't be read is left out, so every line is new against it.
pub fn commit_findings(
    repo: &gix::Repository,
    id: ObjectId,
    unpublished: bool,
    m: &Matcher,
    own: &[String],
) -> Result<Vec<Finding>, Box<dyn Error>> {
    let hash = repo.object_hash();
    let raw = read(repo, id)?.data;
    let commit = CommitRefIter::from_bytes(&raw, hash);
    let tree = commit.clone().tree_id()?;
    let mut parent_trees = Vec::new();
    for p in commit.parent_ids() {
        if let Ok(o) = read(repo, p)
            && let Ok(t) = CommitRefIter::from_bytes(&o.data, hash).tree_id()
        {
            parent_trees.push(t);
        }
    }
    let object = format!("commit {}", id.to_hex_with_len(7));
    let mut found = Vec::new();
    for item in commit_items(repo, tree, &parent_trees)? {
        found.extend(item_findings(m, &object, &item, IN_FILE));
    }
    found.extend(object_findings(&raw, id, unpublished, m, own));
    Ok(found)
}

// Who a tag names and what its message says, and the whole of the blob or
// tree it points at, if it does: that is sent as is.
pub fn tag_findings(
    repo: &gix::Repository,
    id: ObjectId,
    m: &Matcher,
    own: &[String],
) -> Result<Vec<Finding>, Box<dyn Error>> {
    let raw = read(repo, id)?.data;
    let mut found = object_findings(&raw, id, false, m, own);
    let target = TagRefIter::from_bytes(&raw, repo.object_hash()).target_id()?;
    found.extend(blob_findings(repo, target, m)?);
    Ok(found)
}

// A blob, read whole, or a tree: every file under it, by name and content,
// since the push sends them all. A gitlink is a commit in another repo, so it
// is skipped. Any other object gives nothing.
fn blob_findings(
    repo: &gix::Repository,
    id: ObjectId,
    m: &Matcher,
) -> Result<Vec<Finding>, Box<dyn Error>> {
    let o = read(repo, id)?;
    let object = format!("{} {}", o.kind, id.to_hex_with_len(7));
    let mut items = Vec::new();
    match o.kind {
        Kind::Blob => items.push(blob_item(&id.to_string(), &o.data)),
        Kind::Tree => {
            for e in repo.find_tree(id)?.traverse().breadthfirst.files()? {
                if e.mode.is_blob_or_symlink() {
                    let path = e.filepath.to_string();
                    items.push(blob_item(&path, &read(repo, e.oid)?.data));
                    items.push(name_item("file", path));
                }
            }
        }
        _ => {}
    }
    let mut found = Vec::new();
    for item in &items {
        found.extend(item_findings(m, &object, item, IN_FILE));
    }
    Ok(found)
}

// Everything a push sends that names you: its commits, tags, and blobs, and
// the names it gives the remote's refs. The local name never leaves.
pub fn push_findings(
    repo: &gix::Repository,
    out: &Outgoing,
    refs: &[RefLine],
    m: &Matcher,
    own: &[String],
) -> Result<Vec<Finding>, Box<dyn Error>> {
    let mut found = Vec::new();
    for (id, unpublished) in &out.commits {
        found.extend(commit_findings(repo, *id, *unpublished, m, own)?);
    }
    for id in &out.tags {
        found.extend(tag_findings(repo, *id, m, own)?);
    }
    for id in &out.blobs {
        found.extend(blob_findings(repo, *id, m)?);
    }
    for r in refs {
        for h in m.find_ref(&r.remote_ref) {
            found.push(Finding::new(format!("ref {}", r.remote_ref), &h, IN_REF));
        }
    }
    Ok(found)
}

// Each finding on a line, then each fix once.
pub fn render(f: &[Finding]) -> String {
    let mut out = String::new();
    let mut hints = Vec::new();
    for finding in f {
        out += &format!("{}  {}\n", finding.at, finding.what);
        if !hints.contains(&finding.hint) {
            hints.push(finding.hint);
        }
    }
    for hint in hints {
        out += &format!("fix: {hint}\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::details::Details;

    const ME: &str = "me@home.example";

    fn hints(raw: &str) -> Vec<&'static str> {
        let m = Matcher::new(&Details {
            emails: vec![(ME.into(), "test".into())],
            home: None,
            raw_login: Some("jdoe".into()),
            login: None,
            hosts: vec![],
            weak: vec![],
        });
        let id = ObjectId::null(gix::hash::Kind::Sha1);
        object_findings(raw.as_bytes(), id, true, &m, &[ME.to_string()])
            .iter()
            .map(|f| f.hint)
            .collect()
    }

    #[test]
    fn own_author_gets_the_reset_author_hint() {
        let raw = format!(
            "tree 0\nauthor A <{ME}> 1 +0000\ncommitter C <1+c@users.noreply.github.com> 1 +0000\n\nm\n"
        );
        assert_eq!(hints(&raw), vec![IN_AUTHOR]);
    }

    // A patch applied from someone else keeps them as its author.
    #[test]
    fn own_committer_alone_gets_a_hint_that_keeps_the_author() {
        let raw = format!(
            "tree 0\nauthor B <bob@corp.example> 1 +0000\ncommitter C <{ME}> 1 +0000\n\nm\n"
        );
        assert_eq!(hints(&raw), vec![IN_COMMITTER]);
    }

    #[test]
    fn a_foreign_committer_gets_a_hint_that_keeps_the_author() {
        let raw = "tree 0\nauthor B <bob@corp.example> 1 +0000\ncommitter C <c@corp.example> 1 +0000\n\nm\n";
        assert_eq!(hints(raw), vec![IN_COMMITTER]);
    }

    #[test]
    fn a_tag_gets_the_tag_hint() {
        let raw = format!(
            "object 0\ntype commit\ntag v1\ntagger T <{ME}> 1 +0000\n\nfrom /Users/jdoe/x\n"
        );
        assert_eq!(hints(&raw), vec![IN_TAG, IN_TAG]);
    }

    // Only a signature's base64 is dropped, not lines a merged tag quotes.
    #[test]
    fn a_quoted_block_in_a_merged_tag_is_matched() {
        let raw = "tree 0\nmergetag object o\n type commit\n tag v1\n tagger T <1+t@users.noreply.github.com> 1 +0000\n \n v1\n -----BEGIN CERTIFICATE-----\n /Users/jdoe/x\n -----END CERTIFICATE-----\nauthor A <1+a@users.noreply.github.com> 1 +0000\ncommitter C <1+c@users.noreply.github.com> 1 +0000\n\nm\n";
        assert_eq!(hints(raw), vec![IN_HEADER]);
    }

    #[test]
    fn an_identity_line_without_an_email_is_matched() {
        let raw = "tree 0\nauthor /Users/jdoe 1 +0000\ncommitter C <1+c@users.noreply.github.com> 1 +0000\n\nm\n";
        assert_eq!(hints(raw), vec![IN_HEADER]);
    }

    #[test]
    fn quotes_any_reads_past_the_kind_and_case() {
        let h = Hit::new(1, HitKind::Login, "JDoe@jdoes-laptop.local".to_string());
        let f = Finding::new("x".to_string(), &h, IN_FILE);
        assert!(quotes_any(&f, &["jdoe@jdoes-laptop.local".to_string()]));
    }

    #[test]
    fn render_gives_each_hint_once() {
        let h = Hit::new(1, HitKind::HomePath, "/Users/zebra".to_string());
        let f = |at: &str| Finding::new(at.to_string(), &h, IN_FILE);
        assert_eq!(
            render(&[f("commit 1 a:1"), f("commit 1 b:2")]),
            format!(
                "commit 1 a:1  home path /Users/zebra\ncommit 1 b:2  home path /Users/zebra\nfix: {IN_FILE}\n"
            )
        );
    }
}
