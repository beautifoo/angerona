use crate::{
    check::{
        Finding, IN_FILE, IN_REF, commit_findings, item_findings, quotes_any, tag_findings, what,
    },
    identity::machine_guessed,
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
    // HEAD is no ref: a detached one reaches commits no branch does.
    if !repo.head()?.is_unborn() {
        stack.push(repo.head_id()?.detach());
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

// One address as the commit and tag identities name it: per field, how many
// commits or tags, and the first and last object, in the order scan walked
// them (refs in name order, then a stack, so not by date).
struct Group {
    email: String,
    mine: bool,
    fields: Vec<(String, &'static str, usize)>,
    first: String,
    last: String,
    hints: Vec<&'static str>,
}

// Where an identity finding sits: the object's kind and short hash, and the
// field. Any other place (a message line, a file) has more words or a colon.
fn ident_place(at: &str) -> Option<(&'static str, &str, &str)> {
    let mut words = at.split(' ');
    let (kind, hash, field) = (words.next()?, words.next()?, words.next()?);
    let kind = match kind {
        "commit" => "commit",
        "tag" => "tag",
        _ => return None,
    };
    if words.next().is_some() || field.contains(':') {
        return None;
    }
    Some((kind, hash, field))
}

// Why: on a team repo, thousands of other people's emails bury the findings.
// Drops another person's email unless it looks machine-guessed from your login,
// so your own old machine (or all_emails), and an empty one always; then folds
// the identity emails into one group per address, yours first, then by count,
// highest first. The rest stay as lines.
fn group(
    all: Vec<Finding>,
    own: &[String],
    login: Option<&str>,
    all_emails: bool,
) -> (Vec<Finding>, Vec<Group>) {
    let mut plain = Vec::new();
    let mut groups: Vec<Group> = Vec::new();
    for f in all {
        let Some(email) = f.what.strip_prefix("email ").map(str::to_string) else {
            plain.push(f);
            continue;
        };
        let mine = own.iter().any(|e| e.to_lowercase() == email.to_lowercase());
        let local = email.rsplit_once('@').map(|(l, _)| l.to_lowercase());
        let guessed = machine_guessed(&email) && login.map(str::to_lowercase) == local;
        if email.is_empty() || (!mine && !all_emails && !guessed) {
            continue;
        }
        let Some((kind, hash, field)) = ident_place(&f.at) else {
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
                    first: hash.to_string(),
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
            .find(|(n, k, _)| n == field && *k == kind)
        {
            Some(entry) => entry.2 += 1,
            None => g.fields.push((field.to_string(), kind, 1)),
        }
        g.last = hash.to_string();
        if !g.hints.contains(&f.hint) {
            g.hints.push(f.hint);
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
// each fix once.
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
    for hint in hints {
        out += &format!("fix: {hint}\n");
    }
    out
}

// angerona scan: everything in the repo that names you, with nothing changed,
// less what quotes an address in skip. Any error reading the repo fails the
// scan, never a clean result.
pub fn run(
    repo: &gix::Repository,
    m: &Matcher,
    own: &[String],
    skip: &[String],
    login: Option<&str>,
    all_emails: bool,
) -> i32 {
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
            let all: Vec<Finding> = history
                .into_iter()
                .chain(current)
                .filter(|f| !quotes_any(f, skip))
                .collect();
            let (plain, groups) = group(all, own, login, all_emails);
            if plain.is_empty() && groups.is_empty() {
                return 0;
            }
            let from_history =
                !groups.is_empty() || plain.iter().any(|f| !f.at.starts_with("current "));
            print!("{}", render_scan(&plain, &groups));
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
