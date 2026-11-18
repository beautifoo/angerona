use crate::{
    check::{Finding, IN_FILE, IN_REF, commit_findings, item_findings, render, tag_findings, what},
    matcher::Matcher,
    push::{parents, read},
    text::{blob_item, name_item},
};
use gix::{ObjectId, objs::Kind};
use std::{
    collections::HashSet,
    error::Error,
    time::{Duration, Instant},
};

const FILTER: &str = "fix history with git filter-repo (keep its mailmap file outside the repo)";
const PARTIAL: &str = "angerona: this clone is shallow or partial; run git fetch --unshallow \
                       (or a full clone) first, or the report misses old commits";

// Whether the clone lacks old commits: shallow, or partial by its config.
fn incomplete(repo: &gix::Repository, shallow: &HashSet<ObjectId>) -> bool {
    let config = repo.config_snapshot();
    let promisor = repo.remote_names().iter().any(|n| {
        config
            .boolean(format!("remote.{n}.promisor").as_str())
            .unwrap_or(false)
    });
    !shallow.is_empty() || promisor || config.string("extensions.partialclone").is_some()
}

// Every commit and tag reachable from any ref, each once, and every ref name,
// as unpublished; then each file of HEAD, whole.
fn findings(
    repo: &gix::Repository,
    m: &Matcher,
    own: &[String],
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
            history.push(Finding {
                at: format!("ref {name}"),
                what: what(&h),
                hint: IN_REF,
            });
        }
        if let Some(id) = r.target().try_id() {
            stack.push(id.to_owned());
        }
    }
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
                history.extend(commit_findings(repo, id, true, m, own)?);
                stack.extend(parents(id, &o.data, shallow, hash));
                commits += 1;
                if last.elapsed() >= Duration::from_secs(2) {
                    eprintln!("angerona: scanned {commits} commits...");
                    last = Instant::now();
                }
            }
            Kind::Tag => {
                history.extend(tag_findings(repo, id, m, own)?);
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
                    IN_FILE,
                ));
                current.extend(item_findings(
                    m,
                    "current",
                    &name_item("file", path),
                    IN_FILE,
                ));
            }
        }
    }
    Ok((history, current))
}

// angerona scan: everything in the repo that names you, with nothing changed.
// Any error reading the repo fails the scan, never a clean result.
pub fn run(repo: &gix::Repository, m: &Matcher, own: &[String]) -> i32 {
    let shallow: HashSet<ObjectId> = match repo.shallow_commits() {
        Ok(ids) => ids.map(|v| v.iter().copied().collect()).unwrap_or_default(),
        Err(e) => {
            eprintln!("angerona: {e}");
            return 1;
        }
    };
    if incomplete(repo, &shallow) {
        println!("{PARTIAL}");
    }
    match findings(repo, m, own, &shallow) {
        Ok((history, current)) => {
            let from_history = !history.is_empty();
            let all: Vec<Finding> = history.into_iter().chain(current).collect();
            if all.is_empty() {
                return 0;
            }
            print!("{}", render(&all));
            if from_history {
                println!("fix: {FILTER}");
            }
            1
        }
        Err(e) => {
            eprintln!("angerona: {e}");
            1
        }
    }
}
