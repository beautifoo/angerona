use gix::{
    ObjectId,
    objs::{CommitRefIter, Kind, TagRefIter},
    progress::Discard,
    protocol::handshake::Ref,
    remote::{Direction, fetch::Tags, ref_map},
};
use std::collections::HashSet;

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
    let mut fields = line.rsplitn(4, ' ');
    fields.next(); // the remote's sha
    let remote_ref = fields.next().unwrap_or("").to_string();
    let sha = fields.next().unwrap_or("").to_string();
    let local_ref = fields.next().unwrap_or("").to_string();
    if !sha.is_empty() && sha.chars().all(|c| c == '0') {
        return None;
    }
    Some(RefLine {
        local_ref,
        sha,
        remote_ref,
    })
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

// The objects the remote says it holds, and this repo has. It is asked at the
// URL git gives, which git has already rewritten, so it isn't rewritten again,
// or else by its name. Remote-tracking refs are not trusted: they go stale,
// and a remote's URL can change under them. If the remote can't be asked,
// nothing is held, and each pushed ref's whole history is scanned.
pub fn held(repo: &gix::Repository, remote: &str, url: Option<&str>) -> Vec<ObjectId> {
    let target = url.unwrap_or(remote);
    // remote.<name>.vcs names a remote helper too.
    let vcs = repo
        .config_snapshot()
        .string(&format!("remote.{remote}.vcs"))
        .is_some();
    let listed = if may_ask(target) && !vcs {
        list_refs(repo, remote, url)
    } else {
        None
    };
    let Some(refs) = listed else {
        eprintln!(
            "angerona: note: can't ask {} what it holds, scanning the whole history",
            without_credentials(target)
        );
        return Vec::new();
    };
    let mut ids = Vec::new();
    for r in &refs {
        // A tag's ref names the tag and the commit it peels to, as ls-remote does.
        let (_, points_to, peeled) = r.unpack();
        for id in [points_to, peeled].into_iter().flatten() {
            if repo.has_object(id) {
                ids.push(id.to_owned());
            }
        }
    }
    ids
}

fn list_refs(repo: &gix::Repository, remote: &str, url: Option<&str>) -> Option<Vec<Ref>> {
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

fn read(repo: &gix::Repository, id: ObjectId) -> gix::Result<gix::ObjectDetached> {
    Ok(repo.find_object(id)?.detach())
}

// Git takes the commits in .git/shallow to have no parents.
fn parents(
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
