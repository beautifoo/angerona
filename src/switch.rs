use crate::staged;
use gix::{
    bstr::ByteSlice,
    config::{
        File, Source,
        file::{Metadata, init},
        parse::{EventRef, Events},
        source::Kind,
    },
    discover::upwards,
    open::permissions::Config,
};
use std::{
    env, fmt, fs,
    path::{Path, PathBuf},
};

pub enum Switch {
    On,
    // The key that switches the hook off, and where it is set.
    Off { key: String, by: By },
    // Why git's answer can't be read here, with the fix where there is a safe one.
    Unclear(String),
}

// Where a switch-off is set, named as git names it.
pub enum By {
    // The repo's own config: its scope, "local" or "worktree", and its file.
    Repo(&'static str, String),
    File(String),
    // GIT_CONFIG_COUNT, which git shows as "command line:".
    Env,
}

impl fmt::Display for By {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            By::Repo(_, file) | By::File(file) => f.write_str(file),
            By::Env => f.write_str("GIT_CONFIG_COUNT"),
        }
    }
}

pub fn git_bool(value: &str) -> Option<bool> {
    gix::config::Boolean::try_from(value).ok().map(bool::from)
}

// Whether git, in dir, runs hook or skips it. As git's hooks do, it
// reads every config scope but not GIT_CONFIG, the global ones alone outside a
// repo, and takes the last value that is a bool. The system config git reads
// is the one it was built with, which only running git can say: with
// may_run_git it is read, and without, no system config is, so a system-wide
// switch-off reads as on, never one git ignores as off. gix's ceiling check is
// off by one against git's, so with ceiling directories set it could find a
// repo git doesn't; and gix opens a repo owned by someone else, and not in
// safe.directory, with its own config, where git reads no repo at all.
pub fn read(dir: &Path, hook: &str, may_run_git: bool) -> Switch {
    if env::var_os("GIT_CEILING_DIRECTORIES").is_some_and(|v| !v.is_empty()) {
        return Switch::Unclear(
            "as GIT_CEILING_DIRECTORIES is set; fix: run it without GIT_CEILING_DIRECTORIES".into(),
        );
    }
    // Where git -c hands its values to the processes git runs, which gix
    // doesn't read.
    if env::var_os("GIT_CONFIG_PARAMETERS").is_some_and(|v| !v.is_empty()) {
        return Switch::Unclear(
            "as git -c values are set; fix: run it without GIT_CONFIG_PARAMETERS".into(),
        );
    }
    let sources = Config {
        git_binary: may_run_git,
        system: may_run_git,
        ..Config::all()
    };
    let start = match staged::start(dir) {
        Ok(start) => start,
        Err(e) => return Switch::Unclear(format!("as its directory can't be read ({e})")),
    };
    let repo = match staged::open_at(&start, sources) {
        Ok(repo) => Some(repo),
        Err(e)
            if e.downcast_any_ref::<upwards::Error>().is_some_and(|e| {
                matches!(
                    e,
                    upwards::Error::NoGitRepository { .. }
                        | upwards::Error::NoGitRepositoryWithinCeiling { .. }
                        | upwards::Error::NoGitRepositoryWithinFs { .. }
                )
            }) =>
        {
            None
        }
        // A repo git reads but gix can't, such as one with SHA-256 objects.
        Err(e) => {
            return Switch::Unclear(format!(
                "as gix can't read this repo ({})",
                e.probable_cause()
            ));
        }
    };
    // git names the repo's own config from where it started, or, below a
    // worktree's top, from the top.
    let (config, from) = match repo {
        Some(repo) if repo.git_dir_trust() != gix::sec::Trust::Full => {
            return Switch::Unclear("as git doesn't trust this repo".into());
        }
        Some(repo) => (
            repo.config_snapshot().plumbing().clone(),
            [Some(start), repo.workdir().map(Path::to_path_buf)],
        ),
        None => match globals(may_run_git) {
            Some(config) => (config, [None, None]),
            None => return Switch::On,
        },
    };
    rule(&config, &from, hook)
}

// The config git reads outside a repo: the global files and GIT_CONFIG_COUNT,
// with the system config only when git may run to say where it is.
fn globals(may_run_git: bool) -> Option<File> {
    let mut config = if may_run_git {
        File::from_globals().ok()?
    } else {
        let global = Kind::Global.sources().iter().map(|&source| Metadata {
            path: source
                .storage_location(&mut |name| env::var_os(name))
                .filter(|p| p.is_file()),
            source,
            level: 0,
            trust: gix::sec::Trust::Full,
        });
        let home = env::var_os("HOME").map(PathBuf::from);
        let options = init::Options {
            includes: gix::config::file::includes::Options::follow_without_conditional(
                home.as_deref(),
            ),
            ..Default::default()
        };
        File::from_paths_metadata(global, options)
            .ok()?
            .unwrap_or_default()
    };
    config
        .append(File::from_environment_overrides().ok()?)
        .ok()?;
    Some(config)
}

// The repo's own config file as git names it: below GIT_DIR as given, while
// that is set; else from the first of from that holds it; else in full, with
// any ".." folded, as a linked worktree's shared config.
fn named(file: &Path, from: &[Option<PathBuf>]) -> String {
    let file = staged::fold(file);
    let given = env::var_os("GIT_DIR").map(PathBuf::from);
    given
        .and_then(|dir| {
            // gix and the cwd may spell the same directory by different links.
            let real = fs::canonicalize(&file).ok()?;
            let rest = real.strip_prefix(fs::canonicalize(&dir).ok()?).ok()?;
            Some(dir.join(rest))
        })
        .or_else(|| {
            from.iter()
                .flatten()
                .find_map(|base| file.strip_prefix(base).ok().map(Path::to_path_buf))
        })
        .unwrap_or(file)
        .display()
        .to_string()
}

fn rule(config: &File, from: &[Option<PathBuf>], hook: &str) -> Switch {
    let by = |meta: &Metadata| match (&meta.path, meta.source) {
        (Some(p), Source::Local | Source::Worktree) => By::Repo(
            if meta.source == Source::Local {
                "local"
            } else {
                "worktree"
            },
            named(p, from),
        ),
        (Some(p), _) => By::File(p.display().to_string()),

        (None, _) => By::Env,
    };
    let mut by_hook = None;
    for section in config.sections_by_name("hook").into_iter().flatten() {
        let Some(name) = section.header().subsection_name() else {
            continue;
        };
        // git fails on a hook's key with no "=", which gix reads as an empty
        // value: only its events tell the two apart.
        let text = section.to_bstring();
        let events = Events::from_bytes(&text, None);
        let mut events = events
            .iter()
            .flat_map(|e| e.iter())
            .filter(|e| !matches!(e, EventRef::Whitespace(_)));
        while let Some(e) = events.next() {
            if let EventRef::SectionValueName(key) = e
                && !matches!(events.next(), Some(EventRef::KeyValueSeparator))
            {
                return Switch::Unclear(format!(
                    "as {} has hook.{name}.{key} with no value, which git can't read; fix: give it one",
                    by(section.meta())
                ));
            }
        }
        let last = section
            .values("enabled")
            .iter()
            .rev()
            .find_map(|v| git_bool(&v.to_str_lossy()));
        // In the old [hook.Name] syntax git lowercases the name.
        let name = if text.get(5) == Some(&b'.') {
            name.to_ascii_lowercase()
        } else {
            name.to_vec()
        };
        // git 2.54 has no hook.<event>.enabled, so only the hook's own key
        // switches it off on every git angerona supports.
        if name != hook.as_bytes() {
            continue;
        }
        if let Some(on) = last {
            by_hook = Some((on, section.meta()));
        }
    }
    let Some((false, meta)) = by_hook else {
        return Switch::On;
    };
    Switch::Off {
        key: format!("hook.{hook}.enabled"),
        by: by(meta),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_bools_as_git_does() {
        for v in ["false", "no", "off", "0", "0x0", "0k", ""] {
            assert_eq!(git_bool(v), Some(false), "{v}");
        }
        for v in ["true", "yes", "on", "1", "2", "1k"] {
            assert_eq!(git_bool(v), Some(true), "{v}");
        }
        assert_eq!(git_bool("maybe"), None);
    }
}
