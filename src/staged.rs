use crate::scan::{Item, Label, blob_item};
use gix::{index::entry::Mode, open::permissions::Config};
use std::{
    env, fs,
    ops::ControlFlow,
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
};

// Where git starts looking for the repo from dir, by the path an includeIf
// "gitdir:" sees, which gix matches only as given or as its real path. git
// names the repo by $PWD, while that names the cwd, at the top of a worktree
// whose .git is a directory; below it git moves to the top, where $PWD no
// longer names its cwd, and a .git file names a real path. Elsewhere, then,
// the real path.
pub fn start(dir: &Path) -> std::io::Result<PathBuf> {
    let ino = |p: &Path| fs::metadata(p).map(|m| (m.dev(), m.ino())).ok();
    let cwd = env::var_os("PWD")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && ino(p).is_some() && ino(p) == ino(Path::new(".")))
        .map_or_else(env::current_dir, Ok)?;
    let named = std::path::absolute(cwd.join(dir))?;
    if named.join(".git").is_dir() {
        Ok(named)
    } else {
        Ok(fs::canonicalize(&named).unwrap_or(named))
    }
}

// The path with each ".." folded into the part before it, without reading the
// filesystem, as git names a path it was handed that way.
pub fn fold(path: &Path) -> PathBuf {
    path.components().fold(PathBuf::new(), |mut out, part| {
        if part == Component::ParentDir {
            out.pop();
        } else {
            out.push(part);
        }
        out
    })
}

// The repository git would use from start, as a hook sees it, reading the
// config sources config allows: GIT_DIR and GIT_INDEX_FILE, which git sets for
// the temporary index of commit -a or commit <paths>, are honoured. Replace
// objects are off. Like git, ceiling directories that don't cover start are
// ignored, not an error.
pub fn open_at(start: &Path, config: Config) -> Result<gix::Repository, gix::Error> {
    let mut trust = gix::sec::trust::Mapping::<gix::open::Options>::default();
    trust.full.permissions.config = config;
    let mut repo: gix::Repository =
        gix::ThreadSafeRepository::discover_with_environment_overrides_opts(
            start,
            gix::discover::upwards::Options {
                match_ceiling_dir_or_error: false,
                ..Default::default()
            },
            trust,
        )?
        .into();
    repo.objects.ignore_replacements = true;
    Ok(repo)
}

// The repository git would use in dir, with every config source but the git
// binary's own, which only running git can find.
pub fn open(dir: &Path) -> Result<gix::Repository, String> {
    open_at(&start(dir).map_err(|e| e.to_string())?, Config::all()).map_err(|e| e.to_string())
}

// What the commit adds: the identity it will carry, each staged file in full,
// then the staged files' names, submodules' too, each its own item so that two
// names never spell a term between them.
pub fn items(repo: &gix::Repository) -> Result<Vec<Item>, String> {
    let err = |e: gix::Error| e.to_string();
    let tree = repo.head_tree_id_or_empty().map_err(err)?.detach();
    let index = repo.index_or_empty().map_err(err)?;
    // Without patterns this matches every path, not only those under the cwd.
    let mut everything = repo
        .pathspec(
            false,
            None::<&str>,
            false,
            &gix::index::State::new(repo.object_hash()),
            gix::worktree::stack::state::attributes::Source::IdMapping,
        )
        .map_err(err)?;
    let mut staged = Vec::new();
    repo.tree_index_status(
        &tree,
        &index,
        Some(&mut everything),
        gix::status::tree_index::TrackRenames::Disabled,
        |change, _, _| {
            if !matches!(change, gix::diff::index::ChangeRef::Deletion { .. }) {
                let (path, _, mode, id) = change.fields();
                staged.push((path.to_string(), mode, id.to_owned()));
            }
            Ok(ControlFlow::Continue(()))
        },
    )
    .map_err(err)?;

    let var = |name| env::var_os(name).map(|v| v.to_string_lossy().into_owned());
    let ident: Vec<_> = [
        var("GIT_AUTHOR_NAME"),
        var("GIT_AUTHOR_EMAIL").map(|e| format!("<{e}>")),
    ]
    .into_iter()
    .flatten()
    .collect();
    let mut items = Vec::new();
    if !ident.is_empty() {
        items.push(Item {
            label: Label::Plain("identity".into()),
            bytes: ident.join(" ").into_bytes(),
        });
    }
    for (path, mode, id) in &staged {
        // A submodule's entry is a commit in another repo, not a file here.
        if *mode != Mode::COMMIT {
            items.push(blob_item(path, &repo.find_blob(*id).map_err(err)?.data));
        }
    }
    for (path, ..) in staged {
        items.push(Item {
            bytes: path.clone().into_bytes(),
            label: Label::Name("file", path),
        });
    }
    Ok(items)
}
