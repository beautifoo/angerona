use gix::{
    ObjectId,
    objs::{CommitRefIter, Kind, TagRefIter},
    progress::Discard,
    protocol::handshake::Ref,
    remote::{Direction, fetch::Tags, ref_map},
};
use std::{collections::HashSet, error::Error};

pub struct RefLine {
    pub sha: String,
    pub remote_ref: String,
}

// A pre-push line, "<local ref> <sha> <remote ref> <remote sha>", where only
// the local ref, as the user typed it, can hold a space: so it is read from
// the right, and the local ref, which never leaves, is not kept. A deletion, whose sha is all zeros, publishes nothing and gives
// None. (Its local ref reads "(delete)", but a ref can have that name.) A line
// too short to hold a sha gives an empty one, which the walk refuses.
pub fn parse_ref_line(line: &str) -> Option<RefLine> {
    let mut fields = line.rsplitn(4, ' ');
    fields.next(); // the remote's sha
    let remote_ref = fields.next().unwrap_or("").to_string();
    let sha = fields.next().unwrap_or("").to_string();
    if !sha.is_empty() && sha.chars().all(|c| c == '0') {
        return None;
    }
    Some(RefLine { sha, remote_ref })
}

// A commit as stored, less its signature's base64 lines, which can spell a
// short term by chance: those of a gpgsig header, before the blank line that
// ends the headers. The signature's other lines, such as a Comment: one, stay.
pub fn strip_signature(commit: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut in_headers = true;
    let mut in_signature = false;
    for line in commit.split_inclusive(|&b| b == b'\n') {
        let text = line.strip_suffix(b"\n").unwrap_or(line);
        if text.is_empty() {
            in_headers = false;
        }
        if in_headers && (text.starts_with(b"gpgsig ") || text.starts_with(b"gpgsig-sha256 ")) {
            in_signature = true;
            continue;
        }
        // A header goes on over the lines that start with a space.
        if !text.starts_with(b" ") {
            in_signature = false;
        }
        if in_signature
            && text[1..]
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(b))
        {
            continue;
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

// The objects the remote holds, and this repo has: the ids the public check
// was given; else the remote's refs, listed over the push transport at the
// URL git gives, which git has already rewritten, so it isn't rewritten
// again; else its remote-tracking refs, which can be stale. If all of them
// fail, nothing is held, and each pushed ref's whole history is scanned.
pub fn held(
    repo: &gix::Repository,
    remote: &str,
    url: &str,
    advert: Option<Vec<ObjectId>>,
) -> Vec<ObjectId> {
    let ids = advert
        .or_else(|| listed(repo, remote, url))
        .or_else(|| tracking(repo, remote));
    let Some(ids) = ids else {
        eprintln!(
            "angerona: note: can't ask {} what it holds, scanning the whole history",
            without_credentials(url)
        );
        return Vec::new();
    };
    let mut held = Vec::new();
    for id in ids {
        if repo.has_object(id) {
            held.push(id);
        }
    }
    held
}

// The remote's refs over the push transport. A tag's ref names the tag and
// the commit it peels to, as ls-remote does.
fn listed(repo: &gix::Repository, remote: &str, url: &str) -> Option<Vec<ObjectId>> {
    // remote.<name>.vcs names a remote helper too.
    let vcs = repo
        .config_snapshot()
        .string(&format!("remote.{remote}.vcs"))
        .is_some();
    if !may_ask(url) || vcs {
        return None;
    }
    let mut ids = Vec::new();
    for r in &list_refs(repo, url)? {
        let (_, points_to, peeled) = r.unpack();
        for id in [points_to, peeled].into_iter().flatten() {
            ids.push(id.to_owned());
        }
    }
    Some(ids)
}

// The ids of refs/remotes/<remote>/*; none is nothing to go by.
fn tracking(repo: &gix::Repository, remote: &str) -> Option<Vec<ObjectId>> {
    let ids = ref_ids(repo, &format!("refs/remotes/{remote}/")).ok()?;
    (!ids.is_empty()).then_some(ids)
}

// The ids of the refs under prefix that name an object, not another ref.
fn ref_ids(repo: &gix::Repository, prefix: &str) -> Result<Vec<ObjectId>, Box<dyn Error>> {
    let mut ids = Vec::new();
    for r in repo.references()?.prefixed(prefix)? {
        ids.extend(r?.try_id().map(|id| id.detach()));
    }
    Ok(ids)
}

fn list_refs(repo: &gix::Repository, url: &str) -> Option<Vec<Ref>> {
    let r = repo.remote_at_without_url_rewrite(url).ok()?;
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
}

// Git gives the URL with its credentials, which can sit in the user field
// too: a URL is shown without either.
pub fn without_credentials(url: &str) -> String {
    match gix::url::parse(url) {
        Ok(mut parsed) => {
            parsed.set_user(None);
            parsed.set_password(None);
            parsed.to_bstring().to_string()
        }
        Err(_) => url.to_string(),
    }
}

pub fn read(repo: &gix::Repository, id: ObjectId) -> gix::Result<gix::ObjectDetached> {
    Ok(repo.find_object(id)?.detach())
}

// Git takes the commits in .git/shallow to have no parents.
pub fn parents(
    id: ObjectId,
    commit: &[u8],
    shallow: &HashSet<ObjectId>,
    hash: gix::hash::Kind,
) -> Vec<ObjectId> {
    if shallow.contains(&id) {
        return Vec::new();
    }
    CommitRefIter::from_bytes(commit, hash)
        .parent_ids()
        .collect()
}

// What a push sends. `bool` says a commit is unpublished: no remote-tracking
// ref of any remote reaches it. `blobs` are the blobs and trees a pushed ref
// names itself; a tag's own target is read with the tag.
pub struct Outgoing {
    pub commits: Vec<(ObjectId, bool)>,
    pub tags: Vec<ObjectId>,
    pub blobs: Vec<ObjectId>,
}

// The commits reachable from the pushed shas and not from what the remote
// holds, as `git rev-list <tips> ^<held>` lists them, and every tag object on
// the way, even one whose commit the remote holds. gix ignores grafts, so a
// repo with them must be refused before this.
pub fn outgoing(
    repo: &gix::Repository,
    refs: &[RefLine],
    held: &[ObjectId],
) -> Result<Outgoing, Box<dyn Error>> {
    let hash = repo.object_hash();
    let shallow: HashSet<ObjectId> = repo
        .shallow_commits()?
        .map(|ids| ids.iter().copied().collect())
        .unwrap_or_default();
    let remote = remote_commits(repo, held, &shallow);
    let published = remote_commits(repo, &ref_ids(repo, "refs/remotes/")?, &shallow);

    let mut out = Outgoing {
        commits: Vec::new(),
        tags: Vec::new(),
        blobs: Vec::new(),
    };
    // Each object to read, and whether a pushed ref names it itself.
    let mut stack = Vec::new();
    for r in refs {
        stack.push((ObjectId::from_hex(r.sha.as_bytes())?, true));
    }
    let mut seen = HashSet::new();
    while let Some((id, named)) = stack.pop() {
        if remote.contains(&id) || !seen.insert(id) {
            continue;
        }
        let o = read(repo, id)?;
        match o.kind {
            Kind::Tag => {
                out.tags.push(id);
                stack.push((TagRefIter::from_bytes(&o.data, hash).target_id()?, false));
            }
            Kind::Commit => {
                out.commits.push((id, !published.contains(&id)));
                for p in parents(id, &o.data, &shallow, hash) {
                    stack.push((p, false));
                }
            }
            Kind::Blob | Kind::Tree => {
                if named {
                    out.blobs.push(id);
                }
            }
        }
    }
    Ok(out)
}

// Every commit the remote holds that this repo has. What can't be read is
// left out, as git does, which scans more, never less.
// ponytail: walks the remote's whole history; a date-ordered walk that stops
// once only held commits remain, as git's does, if that proves slow.
fn remote_commits(
    repo: &gix::Repository,
    held: &[ObjectId],
    shallow: &HashSet<ObjectId>,
) -> HashSet<ObjectId> {
    let hash = repo.object_hash();
    let mut remote = HashSet::new();
    let mut stack = held.to_vec();
    while let Some(id) = stack.pop() {
        if remote.contains(&id) {
            continue;
        }
        let Ok(o) = read(repo, id) else { continue };
        match o.kind {
            Kind::Tag => {
                if let Ok(target) = TagRefIter::from_bytes(&o.data, hash).target_id() {
                    stack.push(target);
                }
            }
            Kind::Commit => {
                remote.insert(id);
                stack.extend(parents(id, &o.data, shallow, hash));
            }
            _ => {}
        }
    }
    remote
}

#[cfg(test)]
mod tests {
    use super::*;

    const Z: &str = "0000000000000000000000000000000000000000";

    #[test]
    fn reads_ref_lines_from_the_right() {
        let r = parse_ref_line(&format!("refs/heads/a b 1234567 refs/heads/x {Z}")).unwrap();
        assert_eq!(
            (r.sha.as_str(), r.remote_ref.as_str()),
            ("1234567", "refs/heads/x")
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
        let c = b"tree t\ngpgsig -----BEGIN PGP SIGNATURE-----\n Comment: kept zebra\n iQEzBAAB+/=\n -----END PGP SIGNATURE-----\nauthor A\n\nmsg gpgsig body\n";
        assert_eq!(
            strip_signature(c),
            b"tree t\n Comment: kept zebra\n -----END PGP SIGNATURE-----\nauthor A\n\nmsg gpgsig body\n"
        );
    }
}
