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
const IN_IDENTITY: &str =
    "git rebase -i <base> --exec 'git commit --amend --no-edit --reset-author'";
const IN_MESSAGE: &str = "git rebase -i, reword";
pub const IN_REF: &str = "push under another name: git push <remote> HEAD:refs/heads/<name>";

// One personal detail, where it is, and how to take it out.
pub struct Finding {
    pub at: String,
    pub what: String,
    pub hint: &'static str,
}

// The kind of detail, and the text as found, so it can be searched for.
pub fn what(h: &Hit) -> String {
    let kind = match h.kind {
        HitKind::Email => "email",
        HitKind::HomePath => "home path",
        HitKind::Login => "login",
        HitKind::Host => "machine name",
    };
    format!("{kind} {}", h.text)
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
        found.push(Finding {
            at: format!("{object} {place}"),
            what: what(&h),
            hint,
        });
    }
    found
}

// Who a commit or tag names, and what its message says. A trailer is part of
// the message, so it is found there, with the hint that fixes a message; the
// header identities are matched on their own. Another person's email counts
// only where identity::foreign says.
fn object_findings(
    raw: &[u8],
    object: &str,
    unpublished: bool,
    m: &Matcher,
    own: &[String],
) -> Vec<Finding> {
    let o = identity::read(raw);
    let mut found = Vec::new();
    for i in &o.idents {
        if ["author", "committer", "tagger"].contains(&i.field.as_str()) {
            let item = Item {
                label: Label::Plain(i.field.clone()),
                bytes: i.raw.clone().into_bytes(),
            };
            found.extend(item_findings(m, object, &item, IN_IDENTITY));
        }
    }
    let message = Item {
        label: Label::Lines("message "),
        bytes: o.message.clone(),
    };
    found.extend(item_findings(m, object, &message, IN_MESSAGE));
    for i in foreign(&o, unpublished, own) {
        found.push(Finding {
            at: format!("{object} {}", i.field),
            what: format!("email {}", i.email),
            hint: IN_IDENTITY,
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
    found.extend(object_findings(&raw, &object, unpublished, m, own));
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
    let object = format!("tag {}", id.to_hex_with_len(7));
    let mut found = object_findings(&raw, &object, false, m, own);
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
            found.push(Finding {
                at: format!("ref {}", r.remote_ref),
                what: what(&h),
                hint: IN_REF,
            });
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

    #[test]
    fn render_gives_each_hint_once() {
        let f = |at: &str| Finding {
            at: at.to_string(),
            what: "home path /Users/zebra".to_string(),
            hint: IN_FILE,
        };
        assert_eq!(
            render(&[f("commit 1 a:1"), f("commit 1 b:2")]),
            format!(
                "commit 1 a:1  home path /Users/zebra\ncommit 1 b:2  home path /Users/zebra\nfix: {IN_FILE}\n"
            )
        );
    }
}
