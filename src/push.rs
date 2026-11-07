use crate::scan::{Item, Label, blob_item, fail};
use gix::{
    ObjectId,
    objs::{CommitRefIter, Kind, TagRefIter, TreeRefIter},
    progress::Discard,
    remote::{Direction, fetch::Tags, ref_map},
};
use std::{
    collections::{BTreeSet, HashSet},
    env,
};

pub struct RefLine {
    pub local_ref: String,
    pub sha: String,
    pub remote_ref: String,
}

// A pre-push line, "<local ref> <sha> <remote ref> <remote sha>", where only
// the local ref, as the user typed it, can hold a space: so it is read from
// the right. A deletion, whose sha is all zeros, publishes nothing and gives
// None. (Its local ref reads "(delete)", but a ref can have that name.) A line
// too short to hold a sha gives an empty one, which the walk refuses.
pub fn parse_ref_line(line: &str) -> Option<RefLine> {
    let mut fields = line.rsplitn(4, ' ').skip(1).map(String::from);
    let mut next = || fields.next().unwrap_or_default();
    let (remote_ref, sha, local_ref) = (next(), next(), next());
    (sha.is_empty() || sha.contains(|c| c != '0')).then_some(RefLine {
        local_ref,
        sha,
        remote_ref,
    })
}

// A commit as stored, less its signature's base64 lines, which can spell a
// short term by chance: those of a gpgsig header, before the blank line that
// ends the headers. The signature's other lines, such as a Comment: one, stay.
pub fn strip_signature(commit: &[u8]) -> Vec<u8> {
    let (mut out, mut head, mut sig) = (Vec::new(), true, false);
    for line in commit.split_inclusive(|&b| b == b'\n') {
        let text = line.strip_suffix(b"\n").unwrap_or(line);
        head &= !text.is_empty();
        if head && (text.starts_with(b"gpgsig ") || text.starts_with(b"gpgsig-sha256 ")) {
            sig = true;
            continue;
        }
        if sig && text.starts_with(b" ") {
            if text[1..]
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(b))
            {
                continue;
            }
        } else {
            sig = false;
        }
        out.extend_from_slice(line);
    }
    out
}

// Whether gix reads the URL as git does. It doesn't percent-decode a file://
// path, so it would list another repo (a git:// path with a % is left unasked
// too, which only means more is scanned), and it can't run a remote helper,
// which a "<helper>::" prefix names.
pub fn may_ask(url: &str) -> bool {
    let encoded = (url.starts_with("file://") || url.starts_with("git://")) && url.contains('%');
    let helper = url.find("::").is_some_and(|i| !url[..i].contains('/'));
    !encoded && !helper
}

// The objects the remote says it holds, and this repo has. It is asked at the
// URL git gives, which git has already rewritten, so it isn't rewritten again,
// or else by its name. Remote-tracking refs are not trusted: they go stale,
// and a remote's URL can change under them. If the remote can't be asked,
// nothing is held, and each pushed ref's whole history is scanned.
pub fn held(repo: &gix::Repository, remote: &str, url: Option<&str>) -> Vec<ObjectId> {
    let target = url.unwrap_or(remote);
    let vcs = repo
        .config_snapshot()
        .string(&format!("remote.{remote}.vcs"))
        .is_some();
    let listed = (may_ask(target) && !vcs)
        .then(|| {
            let r = match url {
                Some(url) => repo.remote_at_without_url_rewrite(url).ok()?,
                None => repo.find_fetch_remote(Some(remote.into())).ok()?,
            };
            let options = ref_map::Options {
                prefix_from_spec_as_filter_on_remote: false,
                ..Default::default()
            };
            let (map, _) = r
                .with_fetch_tags(Tags::None)
                .connect(Direction::Fetch)
                .ok()?
                .ref_map(Discard, options)
                .ok()?;
            Some(map.remote_refs)
        })
        .flatten();
    let Some(refs) = listed else {
        // Git gives the URL with its credentials, which can sit in the user
        // field too: a URL is shown without either.
        let shown = gix::url::parse(target).map_or(target.to_string(), |mut u| {
            u.set_user(None);
            u.set_password(None);
            u.to_bstring().to_string()
        });
        eprintln!("angerona: note: can't ask {shown} what it holds, scanning the whole history");
        return Vec::new();
    };
    // A tag's ref names the tag and the commit it peels to, as ls-remote does.
    refs.iter()
        .flat_map(|r| {
            let (_, target, peeled) = r.unpack();
            [target, peeled]
        })
        .flatten()
        .filter(|id| repo.has_object(id))
        .map(ToOwned::to_owned)
        .collect()
}

// What the push sends, as `git rev-list --objects <tips> ^<held>` lists it,
// read as stored: the ref names; each tag and commit; each file version the
// remote lacks; then every name in each new root tree, the remote's files and
// submodules included, since a rename sends only a tree.
pub fn items(
    repo: &gix::Repository,
    refs: &[RefLine],
    held: &[ObjectId],
) -> Result<Vec<Item>, String> {
    if repo.common_dir().join("info/grafts").exists() || env::var_os("GIT_GRAFT_FILE").is_some() {
        fail("this repo has grafts, which gix ignores", 1);
    }
    let hash = repo.object_hash();
    let read = |id: ObjectId| {
        repo.find_object(id)
            .map(|o| o.detach())
            .map_err(|e| e.to_string())
    };
    // Git takes the commits in .git/shallow to have no parents.
    let shallow: HashSet<_> = repo
        .shallow_commits()
        .map_err(|e| e.to_string())?
        .map(|ids| ids.iter().copied().collect())
        .unwrap_or_default();
    let parents = |id: ObjectId, data: &[u8]| -> Vec<_> {
        if shallow.contains(&id) {
            Vec::new()
        } else {
            CommitRefIter::from_bytes(data, hash).parent_ids().collect()
        }
    };

    // Every commit the remote holds that this repo has. What can't be read is
    // left out, as git does, which scans more, never less.
    // ponytail: walks the remote's whole history; a date-ordered walk that stops
    // once only held commits remain, as git's does, if that proves slow.
    let mut remote = HashSet::new();
    let mut stack = held.to_vec();
    while let Some(id) = stack.pop() {
        if remote.contains(&id) {
            continue;
        }
        let Ok(o) = read(id) else { continue };
        match o.kind {
            Kind::Tag => stack.extend(TagRefIter::from_bytes(&o.data, hash).target_id().ok()),
            Kind::Commit => {
                remote.insert(id);
                stack.extend(parents(id, &o.data));
            }
            _ => {}
        }
    }

    let mut items: Vec<_> = refs
        .iter()
        .flat_map(|r| [&r.local_ref, &r.remote_ref])
        .map(|name| Item {
            label: Label::Name("ref", name.clone()),
            bytes: name.clone().into_bytes(),
        })
        .collect();
    // The new commits, from the tips, peeling tags. The held commits met on the
    // way are the boundary, and the trees or blobs the tips peel to are roots.
    let (mut new, mut roots, mut boundary) = (HashSet::new(), Vec::new(), Vec::new());
    let mut stack = refs
        .iter()
        .map(|r| ObjectId::from_hex(r.sha.as_bytes()).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    while let Some(id) = stack.pop() {
        if remote.contains(&id) || !new.insert(id) {
            continue;
        }
        let o = read(id)?;
        match o.kind {
            Kind::Tag => {
                stack.push(
                    TagRefIter::from_bytes(&o.data, hash)
                        .target_id()
                        .map_err(|e| e.to_string())?,
                );
                items.push(Item {
                    label: Label::Plain(format!("tag {id}")),
                    bytes: o.data,
                });
            }
            Kind::Commit => {
                roots.push(
                    CommitRefIter::from_bytes(&o.data, hash)
                        .tree_id()
                        .map_err(|e| e.to_string())?,
                );
                for p in parents(id, &o.data) {
                    if remote.contains(&p) {
                        boundary.push(p);
                    } else {
                        stack.push(p);
                    }
                }
                items.push(Item {
                    label: Label::Plain(format!("commit {id}")),
                    bytes: strip_signature(&o.data),
                });
            }
            Kind::Tree | Kind::Blob => roots.push(id),
        }
    }

    // The remote holds every tree and blob of the boundary commits: git's own
    // approximation of what it holds, which errs toward scanning more.
    let mut seen = HashSet::new();
    let mut stack: Vec<_> = boundary
        .into_iter()
        .filter_map(|id| read(id).ok())
        .filter_map(|o| CommitRefIter::from_bytes(&o.data, hash).tree_id().ok())
        .collect();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Ok(o) = read(id) else { continue };
        for e in TreeRefIter::from_bytes(&o.data, hash).flatten() {
            if e.mode.is_tree() {
                stack.push(e.oid.to_owned());
            } else if e.mode.is_blob_or_symlink() {
                seen.insert(e.oid.to_owned());
            }
        }
    }

    // Each new root tree: every blob or symlink not seen yet, as git skips a
    // seen tree whole, and every name. A name under a seen tree is listed
    // once per (tree, path), which keeps a long history's walk short. A
    // gitlink is a commit in another repo: its name only.
    let (mut names, mut listed) = (BTreeSet::new(), HashSet::new());
    for root in roots {
        if !seen.insert(root) {
            continue;
        }
        let mut stack = vec![(root, String::new(), true)];
        while let Some((id, dir, fresh)) = stack.pop() {
            let o = read(id)?;
            if o.kind == Kind::Blob {
                // A tag can name a blob, which then has no path.
                items.push(blob_item(&id.to_string(), &o.data));
                continue;
            }
            for e in TreeRefIter::from_bytes(&o.data, hash) {
                let e = e.map_err(|e| e.to_string())?;
                let (oid, path) = (e.oid.to_owned(), format!("{dir}{}", e.filename));
                if e.mode.is_tree() {
                    if listed.insert((oid, path.clone())) {
                        stack.push((oid, format!("{path}/"), fresh && seen.insert(oid)));
                    }
                } else if !e.mode.is_commit() && fresh && seen.insert(oid) {
                    items.push(blob_item(&path, &read(oid)?.data));
                }
                names.insert(path);
            }
        }
    }
    items.extend(names.into_iter().map(|path| Item {
        bytes: path.clone().into_bytes(),
        label: Label::Name("file", path),
    }));
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    const Z: &str = "0000000000000000000000000000000000000000";

    #[test]
    fn reads_ref_lines_from_the_right() {
        let r = parse_ref_line(&format!("refs/heads/a b 1234567 refs/heads/x {Z}")).unwrap();
        assert_eq!(
            (r.local_ref.as_str(), r.sha.as_str(), r.remote_ref.as_str()),
            ("refs/heads/a b", "1234567", "refs/heads/x")
        );
        assert!(parse_ref_line(&format!("(delete) {Z} refs/heads/x 1234567")).is_none());
    }

    #[test]
    fn does_not_ask_urls_gix_would_misread() {
        assert!(!may_ask("file:///srv/p%20q.git"));
        assert!(!may_ask("git://host/p%41.git"));
        assert!(!may_ask("codecommit::us-east-1://repo"));
        assert!(may_ask("https://github.com/o/r.git"));
        assert!(may_ask("git@github.com:o/r.git"));
        assert!(may_ask("/srv/plain.git"));
    }

    #[test]
    fn drops_only_a_signatures_base64() {
        let c = b"tree t\ngpgsig -----BEGIN PGP SIGNATURE-----\n Comment: kept globex\n iQEzBAAB+/=\n -----END PGP SIGNATURE-----\nauthor A\n\nmsg gpgsig body\n";
        assert_eq!(
            strip_signature(c),
            b"tree t\n Comment: kept globex\n -----END PGP SIGNATURE-----\nauthor A\n\nmsg gpgsig body\n"
        );
    }
}
