use crate::scan::{Item, Label, blob_item, name_item};
use gix::index::entry::Mode;
use std::{env, error::Error, ops::ControlFlow};

// What the commit adds: the identity it will carry, each staged file in full,
// then the staged files' names, submodules' too, each its own item so that two
// names never spell a term between them.
pub fn items(repo: &gix::Repository) -> Result<Vec<Item>, Box<dyn Error>> {
    let tree = repo.head_tree_id_or_empty()?.detach();
    let index = repo.index_or_empty()?;
    // Without patterns this matches every path, not only those under the cwd.
    let mut everything = repo.pathspec(
        false,
        None::<&str>,
        false,
        &gix::index::State::new(repo.object_hash()),
        gix::worktree::stack::state::attributes::Source::IdMapping,
    )?;
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
    )?;

    let mut ident = Vec::new();
    if let Some(name) = env::var_os("GIT_AUTHOR_NAME") {
        ident.push(name.to_string_lossy().into_owned());
    }
    if let Some(email) = env::var_os("GIT_AUTHOR_EMAIL") {
        ident.push(format!("<{}>", email.to_string_lossy()));
    }
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
            items.push(blob_item(path, &repo.find_blob(*id)?.data));
        }
    }
    for (path, ..) in staged {
        items.push(name_item("file", path));
    }
    Ok(items)
}
