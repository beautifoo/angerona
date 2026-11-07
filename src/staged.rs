use crate::scan::{Item, Label, blob_item};
use gix::index::entry::Mode;
use std::{env, ops::ControlFlow, path::Path};

// The repository git would use in dir, as a hook sees it: GIT_DIR and
// GIT_INDEX_FILE, which git sets for the temporary index of commit -a or
// commit <paths>, are honoured. Replace objects are off. Like git, ceiling
// directories that don't cover dir are ignored, not an error.
pub fn open(dir: &Path) -> Result<gix::Repository, String> {
    let mut repo: gix::Repository =
        gix::ThreadSafeRepository::discover_with_environment_overrides_opts(
            dir,
            gix::discover::upwards::Options {
                match_ceiling_dir_or_error: false,
                ..Default::default()
            },
            Default::default(),
        )
        .map_err(|e| e.to_string())?
        .into();
    repo.objects.ignore_replacements = true;
    Ok(repo)
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
