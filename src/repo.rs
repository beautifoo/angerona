use gix::open::permissions::Config;
use std::{
    env,
    error::Error,
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

// Whether a and b name the same file, by its device and inode.
fn same_file(a: &Path, b: &Path) -> bool {
    let (Ok(a), Ok(b)) = (fs::metadata(a), fs::metadata(b)) else {
        return false;
    };
    a.dev() == b.dev() && a.ino() == b.ino()
}

// Where git starts looking for the repo from dir, by the path an includeIf
// "gitdir:" sees, which gix matches only as given or as its real path. git
// names the repo by $PWD, while that names the cwd, at the top of a worktree
// whose .git is a directory; below it git moves to the top, where $PWD no
// longer names its cwd, and a .git file names a real path. Elsewhere, then,
// the real path.
pub fn start(dir: &Path) -> std::io::Result<PathBuf> {
    let cwd = match env::var_os("PWD").map(PathBuf::from) {
        Some(pwd) if pwd.is_absolute() && same_file(&pwd, Path::new(".")) => pwd,
        _ => env::current_dir()?,
    };
    let named = std::path::absolute(cwd.join(dir))?;
    if named.join(".git").is_dir() {
        Ok(named)
    } else {
        Ok(fs::canonicalize(&named).unwrap_or(named))
    }
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
pub fn open(dir: &Path) -> Result<gix::Repository, Box<dyn Error>> {
    Ok(open_at(&start(dir)?, Config::all())?)
}
