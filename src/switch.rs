use crate::repo;
use gix::{
    bstr::ByteSlice,
    config::{
        File, Source,
        file::{Metadata, includes, includes::conditional, init},
        parse::{EventRef, Events},
        path::interpolate,
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
    // Switched off by the hook's own hook.<name>.enabled, set where By says.
    Off(By),
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

fn git_bool(value: &str) -> Option<bool> {
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
    // Includes are followed below: in a linked worktree gix matches an
    // includeIf "gitdir:" against the main repo's git dir, git against the
    // worktree's own.
    let sources = Config {
        git_binary: may_run_git,
        system: may_run_git,
        includes: false,
        ..Config::all()
    };
    let start = match repo::start(dir) {
        Ok(start) => start,
        Err(e) => return Switch::Unclear(format!("as its directory can't be read ({e})")),
    };
    let repo = match repo::open_at(&start, sources) {
        Ok(repo) => repo,
        Err(e) if is_no_repo(&e) => {
            return match globals(may_run_git) {
                Some(config) => rule(&config, &[], hook),
                None => Switch::On,
            };
        }
        // A repo git reads but gix can't, such as one with SHA-256 objects.
        Err(e) => {
            return Switch::Unclear(format!(
                "as gix can't read this repo ({})",
                e.probable_cause()
            ));
        }
    };
    if repo.git_dir_trust() != gix::sec::Trust::Full {
        return Switch::Unclear("as git doesn't trust this repo".into());
    }

    let mut config = repo.config_snapshot().plumbing().clone();
    let home = env::var_os("HOME").map(PathBuf::from);
    let branch = repo.head_name().ok().flatten();
    let includes = includes::Options::follow(
        interpolate::Context {
            home_dir: home.as_deref(),
            home_for_user: Some(interpolate::home_for_user),
            ..Default::default()
        },
        conditional::Context {
            git_dir: Some(repo.git_dir()),
            branch_name: branch.as_ref().map(AsRef::as_ref),
        },
    );
    let options = init::Options {
        includes,
        ..Default::default()
    };
    if let Err(e) = config.resolve_includes(options) {
        return Switch::Unclear(format!("as its config's includes can't be read ({e})"));
    }
    // git names the repo's own config from where it started, or, below a
    // worktree's top, from the top.
    let mut from = vec![start];
    from.extend(repo.workdir().map(Path::to_path_buf));
    rule(&config, &from, hook)
}

fn is_no_repo(e: &gix::Error) -> bool {
    matches!(
        e.downcast_any_ref::<upwards::Error>(),
        Some(
            upwards::Error::NoGitRepository { .. }
                | upwards::Error::NoGitRepositoryWithinCeiling { .. }
                | upwards::Error::NoGitRepositoryWithinFs { .. }
        )
    )
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
            includes: includes::Options::follow_without_conditional(home.as_deref()),
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
fn named(file: &Path, from: &[PathBuf]) -> String {
    let file = repo::fold_dotdot(file);
    // gix and the cwd may spell the same directory by different links.
    if let Some(dir) = env::var_os("GIT_DIR").map(PathBuf::from)
        && let Ok(real) = fs::canonicalize(&file)
        && let Ok(real_dir) = fs::canonicalize(&dir)
        && let Ok(rest) = real.strip_prefix(&real_dir)
    {
        return dir.join(rest).display().to_string();
    }
    for base in from {
        if let Ok(rest) = file.strip_prefix(base) {
            return rest.display().to_string();
        }
    }
    file.display().to_string()
}

fn set_by(meta: &Metadata, from: &[PathBuf]) -> By {
    match (&meta.path, meta.source) {
        (Some(p), Source::Local) => By::Repo("local", named(p, from)),
        (Some(p), Source::Worktree) => By::Repo("worktree", named(p, from)),
        (Some(p), _) => By::File(p.display().to_string()),
        (None, _) => By::Env,
    }
}

// The first key in a section's text with no "=", which gix reads as an empty
// value and git fails on: only the parsed events tell the two apart.
fn key_without_value(text: &[u8]) -> Option<String> {
    let Ok(parsed) = Events::from_bytes(text, None) else {
        return None;
    };
    let events: Vec<_> = parsed
        .iter()
        .filter(|e| !matches!(e, EventRef::Whitespace(_)))
        .collect();
    for (i, event) in events.iter().enumerate() {
        if let EventRef::SectionValueName(key) = event
            && !matches!(events.get(i + 1), Some(EventRef::KeyValueSeparator))
        {
            return Some(key.to_string());
        }
    }
    None
}

fn rule(config: &File, from: &[PathBuf], hook: &str) -> Switch {
    let mut enabled = None;
    for section in config.sections_by_name("hook").into_iter().flatten() {
        let Some(name) = section.header().subsection_name() else {
            continue;
        };
        // git fails on any hook's key with no value, not only this hook's.
        if let Some(key) = key_without_value(&section.to_bstring()) {
            return Switch::Unclear(format!(
                "as {} has hook.{name}.{key} with no value, which git can't read; fix: give it one",
                set_by(section.meta(), from)
            ));
        }
        // In the old [hook.Name] syntax git lowercases the name.
        let name = if section.header().is_legacy() {
            name.to_ascii_lowercase()
        } else {
            name.to_vec()
        };
        // git 2.54 has no hook.<event>.enabled, so only the hook's own key
        // switches it off on every git angerona supports.
        if name != hook.as_bytes() {
            continue;
        }
        let last = section
            .values("enabled")
            .iter()
            .rev()
            .find_map(|v| git_bool(&v.to_str_lossy()));
        if let Some(on) = last {
            enabled = Some((on, section.meta()));
        }
    }
    match enabled {
        Some((false, meta)) => Switch::Off(set_by(meta, from)),
        _ => Switch::On,
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
