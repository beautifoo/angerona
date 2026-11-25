use crate::{
    check::{Finding, IN_REF, commit_findings, item_findings, tag_findings},
    matcher::{Kind as HitKind, Matcher},
    push::{parents, published, read},
    repo,
    text::{blob_item, name_item},
    verdict::Verdict,
};
use gix::{ObjectId, objs::Kind};
use std::{
    collections::HashSet,
    error::Error,
    time::{Duration, Instant},
};

const FILTER: &str = "fix history with git filter-repo (keep its mailmap file outside the repo)";
const PUSHED: &str =
    "already pushed: fix it in a new commit; rewriting shared history needs everyone who has it";
const IN_CURRENT: &str = "edit the file and commit";
const SHALLOW: &str = "angerona: this clone is shallow; run git fetch --unshallow (or a full \
                       clone) first, or the report misses old commits";
const PARTIAL: &str = "angerona: this clone is partial, and scan never downloads what it lacks; \
                       run git backfill --no-sparse --all (git 2.55 or later) first";

// Every commit and tag reachable from any ref, each once, and every ref name,
// as unpublished, but a commit a remote already has gets the hint for that;
// then each file of HEAD, whole.
fn findings(
    repo: &gix::Repository,
    m: &Matcher,
    shallow: &HashSet<ObjectId>,
) -> Result<(Vec<Finding>, Vec<Finding>), Box<dyn Error>> {
    let hash = repo.object_hash();
    let mut history = Vec::new();
    let mut current = Vec::new();
    let mut stack = Vec::new();
    for r in repo.references()?.all()? {
        let r = r?;
        let name = r.name().as_bstr().to_string();
        for h in m.find_ref(&name) {
            history.push(Finding::new(format!("ref {name}"), &h, IN_REF));
        }
        if let Some(id) = r.target().try_id() {
            stack.push(id.to_owned());
        }
    }
    // HEAD is no ref: a detached one reaches commits no branch does.
    if !repo.head()?.is_unborn() {
        stack.push(repo.head_id()?.detach());
    }
    let published = published(repo, shallow)?;
    let mut seen = HashSet::new();
    let mut commits = 0;
    let mut last = Instant::now();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let o = read(repo, id)?;
        match o.kind {
            Kind::Commit => {
                let mut found = commit_findings(repo, id, true, m)?;
                if published.contains(&id) {
                    found.iter_mut().for_each(|f| f.hint = PUSHED);
                }
                history.extend(found);
                stack.extend(parents(id, &o.data, shallow, hash));
                commits += 1;
                if last.elapsed() >= Duration::from_secs(2) {
                    eprintln!("angerona: scanned {commits} commits...");
                    last = Instant::now();
                }
            }
            Kind::Tag => {
                history.extend(tag_findings(repo, id, m)?);
                stack.push(gix::objs::TagRefIter::from_bytes(&o.data, hash).target_id()?);
            }
            _ => {}
        }
    }
    if !repo.head()?.is_unborn() {
        for e in repo
            .head_commit()?
            .tree()?
            .traverse()
            .breadthfirst
            .files()?
        {
            if e.mode.is_blob_or_symlink() {
                let path = e.filepath.to_string();
                let data = read(repo, e.oid)?.data;
                current.extend(item_findings(
                    m,
                    "current",
                    &blob_item(&path, &data),
                    IN_CURRENT,
                ));
                current.extend(item_findings(
                    m,
                    "current",
                    &name_item("file", path),
                    IN_CURRENT,
                ));
            }
        }
    }
    Ok((history, current))
}

// One address as the commit and tag identities name it: per field, how many
// commits or tags, and the first and last object, in the order scan walked
// them (refs in name order, then a stack, so not by date).
struct Group {
    email: String,
    mine: bool,
    fields: Vec<(String, Kind, usize)>,
    first: String,
    last: String,
    hints: Vec<&'static str>,
}

// Folds the identity emails into one group per address, yours first, then by
// count, highest first. The rest stay as lines.
fn group(all: Vec<Finding>, v: &Verdict) -> (Vec<Finding>, Vec<Group>) {
    let mut plain = Vec::new();
    let mut groups: Vec<Group> = Vec::new();
    for f in all {
        let Some(email) = f.address.clone().filter(|_| f.kind == HitKind::Email) else {
            plain.push(f);
            continue;
        };
        let mine = v.yours(&email);
        let Some((kind, hash, field)) = f.ident else {
            plain.push(f);
            continue;
        };
        let i = match groups.iter().position(|g| g.email == email) {
            Some(i) => i,
            None => {
                groups.push(Group {
                    email,
                    mine,
                    fields: Vec::new(),
                    first: hash.clone(),
                    last: String::new(),
                    hints: Vec::new(),
                });
                groups.len() - 1
            }
        };
        let g = &mut groups[i];
        match g
            .fields
            .iter_mut()
            .find(|(n, k, _)| *n == field && *k == kind)
        {
            Some(entry) => entry.2 += 1,
            None => g.fields.push((field, kind, 1)),
        }
        g.last = hash;
        if !g.hints.contains(&f.hint) {
            g.hints.push(f.hint);
        }
    }
    // One unpublished commit means the address is still fixed by a rewrite.
    for g in &mut groups {
        if g.hints.iter().any(|h| *h != PUSHED) {
            g.hints.retain(|h| *h != PUSHED);
        }
    }
    groups.sort_by_key(|g| {
        (
            !g.mine,
            std::cmp::Reverse(g.fields.iter().map(|f| f.2).sum::<usize>()),
        )
    });
    (plain, groups)
}

// The lines: each non-email finding, then each address on one line, then
// each fix once, and filter-repo last when some history no remote has needs a
// rewrite.
fn render_scan(plain: &[Finding], groups: &[Group]) -> String {
    let mut out = String::new();
    let mut hints = Vec::new();
    for f in plain {
        out += &format!("{}  {}\n", f.at, f.what);
        if !hints.contains(&f.hint) {
            hints.push(f.hint);
        }
    }
    for g in groups {
        let parts: Vec<String> = g
            .fields
            .iter()
            .map(|(field, kind, n)| {
                let s = if *n == 1 { "" } else { "s" };
                format!("{field} in {n} {kind}{s}")
            })
            .collect();
        out += &format!(
            "email {}: {} (first {}, last {})\n",
            g.email,
            parts.join(", "),
            g.first,
            g.last
        );
        for h in &g.hints {
            if !hints.contains(h) {
                hints.push(h);
            }
        }
    }
    if hints.iter().any(|h| ![PUSHED, IN_CURRENT].contains(h)) {
        hints.push(FILTER);
    }
    for hint in hints {
        out += &format!("fix: {hint}\n");
    }
    out
}

// angerona scan: everything in the repo that names you, with nothing changed,
// as far as the verdict keeps it. Any error reading the repo fails the scan,
// never a clean result.
pub fn run(repo: &gix::Repository, m: &Matcher, v: &Verdict) -> i32 {
    let shallow: HashSet<ObjectId> = match repo.shallow_commits() {
        Ok(ids) => ids.map(|v| v.iter().copied().collect()).unwrap_or_default(),
        Err(e) => {
            eprintln!("angerona: {e}");
            return 1;
        }
    };
    if !shallow.is_empty() {
        println!("{SHALLOW}");
    }
    if repo::partial(repo) {
        println!("{PARTIAL}");
    }
    match findings(repo, m, &shallow) {
        Ok((history, current)) => {
            let all: Vec<Finding> = history
                .into_iter()
                .chain(current)
                .filter(|f| v.keeps(f))
                .collect();
            let (plain, groups) = group(all, v);
            if plain.is_empty() && groups.is_empty() {
                return 0;
            }
            print!("{}", render_scan(&plain, &groups));
            1
        }
        Err(e) => {
            eprintln!("angerona: {e}");
            1
        }
    }
}
