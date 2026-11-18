use crate::text::{Item, Label, blob_item, name_item};
use gix::{
    ObjectId,
    bstr::{BString, ByteSlice},
    diff::Options,
    object::tree::diff::ChangeDetached,
};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
};

// The lines of a text blob, without their newlines: a last line with no
// newline counts, the empty tail after the final newline does not.
fn lines(bytes: &[u8]) -> Vec<&[u8]> {
    let mut lines: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

// Lines of `new`, numbered from 1, that no old version has: set membership,
// not a diff, so a line moved or kept from any parent is not new. No old
// versions means every line is new.
pub fn new_lines<'a>(new: &'a [u8], olds: &[&[u8]]) -> Vec<(usize, &'a [u8])> {
    let known: HashSet<&[u8]> = olds.iter().flat_map(|old| lines(old)).collect();
    let mut added = Vec::new();
    for (i, line) in lines(new).into_iter().enumerate() {
        if !known.contains(line) {
            added.push((i + 1, line));
        }
    }
    added
}

// Every blob of a tree by path. A tree that cannot be read (a partial clone
// that never fetched it) has none, so every line against it is new.
fn blobs(repo: &gix::Repository, tree: ObjectId) -> HashMap<BString, ObjectId> {
    let Ok(tree) = repo.find_tree(tree) else {
        return HashMap::new();
    };
    let Ok(entries) = tree.traverse().breadthfirst.files() else {
        return HashMap::new();
    };
    entries
        .into_iter()
        .filter(|e| e.mode.is_blob_or_symlink())
        .map(|e| (e.filepath, e.oid))
        .collect()
}

// What a commit adds, as items to scan: the new lines of each text blob, the
// readable part of each binary one, and the name of each added or renamed
// file. A blob some parent has under any name (a pure rename or move, or a
// merge taking one side whole) adds no lines, and a path some parent has is
// not a new name. A line is new when no old version has it: the same path in
// each parent, or a rename's source.
pub fn commit_items(
    repo: &gix::Repository,
    tree: ObjectId,
    parents: &[ObjectId],
) -> Result<Vec<Item>, Box<dyn Error>> {
    let old_blobs: Vec<_> = parents.iter().map(|p| blobs(repo, *p)).collect();
    let known: HashSet<ObjectId> = old_blobs.iter().flat_map(|b| b.values().copied()).collect();
    let new_tree = repo.find_tree(tree)?;
    let options = Options::default().with_rewrites(Some(Default::default()));
    let mut changes = Vec::new();
    if parents.is_empty() {
        changes.extend(repo.diff_tree_to_tree(None, Some(&new_tree), options)?);
    }
    for parent in parents {
        let old_tree = repo.find_tree(*parent).ok();
        changes.extend(repo.diff_tree_to_tree(old_tree.as_ref(), Some(&new_tree), options)?);
    }
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for change in changes {
        let (location, id, is_blob, source, renamed) = match change {
            ChangeDetached::Addition {
                location,
                entry_mode,
                id,
                ..
            } => (location, id, entry_mode.is_blob_or_symlink(), None, true),
            ChangeDetached::Modification {
                location,
                entry_mode,
                id,
                ..
            } => (location, id, entry_mode.is_blob_or_symlink(), None, false),
            ChangeDetached::Rewrite {
                location,
                entry_mode,
                id,
                source_id,
                ..
            } => (
                location,
                id,
                entry_mode.is_blob_or_symlink(),
                Some(source_id),
                true,
            ),
            ChangeDetached::Deletion { .. } => continue,
        };
        if !is_blob || !seen.insert((location.clone(), id)) {
            continue;
        }
        let path = location.to_str_lossy().into_owned();
        if renamed && !old_blobs.iter().any(|b| b.contains_key(&location)) {
            items.push(name_item("file", path.clone()));
        }
        if known.contains(&id) {
            continue;
        }
        let data = repo.find_object(id)?.detach().data;
        if data[..data.len().min(8000)].contains(&0) {
            items.push(blob_item(&path, &data));
            continue;
        }
        let olds: Vec<Vec<u8>> = old_blobs
            .iter()
            .filter_map(|b| b.get(&location).copied())
            .chain(source)
            .filter_map(|old| Some(repo.find_object(old).ok()?.detach().data))
            .collect();
        let olds: Vec<&[u8]> = olds.iter().map(|o| o.as_slice()).collect();
        for (n, line) in new_lines(&data, &olds) {
            items.push(Item {
                label: Label::Plain(format!("{path}:{n}")),
                bytes: line.to_vec(),
            });
        }
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_lines_in_no_parent() {
        assert_eq!(new_lines(b"a\nb\nc\n", &[b"a\nc\n"]), [(2, &b"b"[..])]);
    }

    #[test]
    fn merge_keeps_only_lines_new_to_every_parent() {
        assert_eq!(new_lines(b"a\nb\nx\n", &[b"a\n", b"b\n"]), [(3, &b"x"[..])]);
    }

    #[test]
    fn root_is_all_new() {
        assert_eq!(new_lines(b"a\nb\n", &[]), [(1, &b"a"[..]), (2, &b"b"[..])]);
    }

    fn git(dir: &std::path::Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
            ])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?} failed");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    #[test]
    fn commit_adds_new_lines_and_names_only() {
        let dir = std::env::temp_dir().join(format!("angerona-added-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(dir.join("keep.txt"), "same\nfile\ncontent\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-qm", "root"]);
        std::fs::write(dir.join("a.txt"), "one\nthree\n").unwrap();
        git(&dir, &["mv", "keep.txt", "moved.txt"]);
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-qm", "next"]);
        let tree = git(&dir, &["rev-parse", "HEAD^{tree}"]).parse().unwrap();
        let parent = git(&dir, &["rev-parse", "HEAD~^{tree}"]).parse().unwrap();
        let repo = gix::open(&dir).unwrap();
        let items = commit_items(&repo, tree, &[parent]).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let shown: Vec<String> = items
            .iter()
            .map(|i| match &i.label {
                Label::Plain(p) => format!("{p} {}", String::from_utf8_lossy(&i.bytes)),
                Label::Name(w, n) => format!("{w} {n}"),
                _ => "other".into(),
            })
            .collect();
        assert_eq!(shown, ["a.txt:2 three", "file moved.txt"]);
    }

    #[test]
    fn symlink_target_and_name_are_added() {
        let dir = std::env::temp_dir().join(format!("angerona-link-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        std::os::unix::fs::symlink("/Users/secretuser/private", dir.join("link")).unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-qm", "root"]);
        let tree = git(&dir, &["rev-parse", "HEAD^{tree}"]).parse().unwrap();
        let repo = gix::open(&dir).unwrap();
        let items = commit_items(&repo, tree, &[]).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let shown: Vec<String> = items
            .iter()
            .map(|i| match &i.label {
                Label::Plain(p) => format!("{p} {}", String::from_utf8_lossy(&i.bytes)),
                Label::Name(w, n) => format!("{w} {n}"),
                _ => "other".into(),
            })
            .collect();
        assert_eq!(shown, ["file link", "link:1 /Users/secretuser/private"]);
    }
}
