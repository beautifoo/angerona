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
    env, fmt,
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
    // GIT_CONFIG_COUNT or GIT_CONFIG_PARAMETERS, which git shows as "command line:".
    Env(&'static str),
}

impl fmt::Display for By {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            By::Repo(_, file) | By::File(file) => f.write_str(file),
            By::Env(var) => f.write_str(var),
        }
    }
}

pub fn git_bool(value: &str) -> Option<bool> {
    gix::config::Boolean::try_from(value).ok().map(bool::from)
}

// Whether git, in dir, runs hook for event or skips it. As git's hooks do, it
// reads every config scope but not GIT_CONFIG, the global ones alone outside a
// repo, and takes the last value that is a bool. The system config git reads
// is the one it was built with, which only running git can say: with
// may_run_git it is read, and without, no system config is, so a system-wide
// switch-off reads as on, never one git ignores as off. gix's ceiling check is
// off by one against git's, so with ceiling directories set it could find a
// repo git doesn't; and gix opens a repo owned by someone else, and not in
// safe.directory, with its own config, where git reads no repo at all.
pub fn read(dir: &Path, hook: &str, event: &str, may_run_git: bool) -> Switch {
    if env::var_os("GIT_CEILING_DIRECTORIES").is_some_and(|v| !v.is_empty()) {
        return Switch::Unclear(
            "as GIT_CEILING_DIRECTORIES is set; fix: run it without GIT_CEILING_DIRECTORIES".into(),
        );
    }
    let sources = Config {
        git_binary: may_run_git,
        system: may_run_git,
        ..Config::all()
    };
    let repo = match staged::start(dir).map(|start| staged::open_at(&start, sources)) {
        Ok(Ok(repo)) => Some(repo),
        Ok(Err(e))
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
        Ok(Err(e)) => {
            return Switch::Unclear(format!(
                "as gix can't read this repo ({})",
                e.probable_cause()
            ));
        }
        Err(e) => return Switch::Unclear(format!("as its directory can't be read ({e})")),
    };
    let (mut config, workdir) = match repo {
        Some(repo) if repo.git_dir_trust() != gix::sec::Trust::Full => {
            return Switch::Unclear("as git doesn't trust this repo".into());
        }
        Some(repo) => (
            repo.config_snapshot().plumbing().clone(),
            repo.workdir().map(Path::to_path_buf),
        ),
        None => match globals(may_run_git) {
            Some(config) => (config, None),
            None => return Switch::On,
        },
    };
    // git reads what git -c set last, and gix doesn't read it at all.
    let given = env::var("GIT_CONFIG_PARAMETERS").unwrap_or_default();
    let cli = parameters(&given).and_then(|entries| command_line(&entries));
    if cli.is_none_or(|cli| config.append(cli).is_err()) {
        return Switch::Unclear(
            "as git can't read GIT_CONFIG_PARAMETERS; fix: unset GIT_CONFIG_PARAMETERS".into(),
        );
    }
    rule(&config, workdir.as_deref(), hook, event)
}

// The entries of GIT_CONFIG_PARAMETERS, where git -c hands its values to the
// processes git runs, as git reads them: shell-quoted words, 'key'='value',
// 'key'= for a key with no value, or from an older git 'key=value' and 'key',
// one space apart. None where git would refuse it as bogus.
fn parameters(given: &str) -> Option<Vec<(String, Option<String>)>> {
    // One quoted word, with '\'' or '\!' for a quote or ! inside it.
    fn word(s: &str) -> Option<(String, &str)> {
        let mut rest = s.strip_prefix('\'')?;
        let mut out = String::new();
        loop {
            let end = rest.find('\'')?;
            out.push_str(&rest[..end]);
            rest = &rest[end + 1..];
            match rest.as_bytes() {
                [b'\\', c @ (b'\'' | b'!'), b'\'', ..] => {
                    out.push(char::from(*c));
                    rest = &rest[3..];
                }
                _ => return Some((out, rest)),
            }
        }
    }
    let mut entries = Vec::new();
    let mut rest = given;
    while !rest.is_empty() {
        let (key, after) = word(rest)?;
        let (entry, after) = match after.strip_prefix('=') {
            Some(after) if after.starts_with('\'') => {
                let (value, after) = word(after)?;
                ((key, Some(value)), after)
            }
            Some(after) => ((key, None), after),
            None => match key.split_once('=') {
                Some((k, v)) => ((k.to_string(), Some(v.to_string())), after),
                None => ((key, None), after),
            },
        };
        entries.push(entry);
        rest = match after.chars().next() {
            None => after,
            Some(c) if c.is_ascii_whitespace() => &after[1..],
            Some(_) => return None,
        };
    }
    Some(entries)
}

// The hook keys among git -c's entries, as config read from the command line.
fn command_line(entries: &[(String, Option<String>)]) -> Option<File> {
    let quote = |s: &str| {
        s.replace('\\', r"\\")
            .replace('"', "\\\"")
            .replace('\n', r"\n")
    };
    let mut text = String::new();
    for (key, value) in entries {
        let Some((section, rest)) = key.split_once('.') else {
            continue;
        };
        let Some((name, var)) = rest.rsplit_once('.') else {
            continue;
        };
        if !section.eq_ignore_ascii_case("hook") {
            continue;
        }
        text += &format!("[hook \"{}\"]\n\t{var}", quote(name));
        if let Some(value) = value {
            text += &format!(" = \"{}\"", quote(value));
        }
        text.push('\n');
    }
    let meta = Metadata {
        path: None,
        source: Source::Cli,
        level: 0,
        trust: gix::sec::Trust::Full,
    };
    File::from_bytes_owned(&mut text.into_bytes(), meta, Default::default()).ok()
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

fn rule(config: &File, workdir: Option<&Path>, hook: &str, event: &str) -> Switch {
    // git shows the repo's own config by its path in the worktree.
    let by = |meta: &Metadata| match (&meta.path, meta.source) {
        (Some(p), Source::Local | Source::Worktree) => By::Repo(
            if meta.source == Source::Local {
                "local"
            } else {
                "worktree"
            },
            workdir
                .and_then(|w| p.strip_prefix(w).ok())
                .unwrap_or(p)
                .display()
                .to_string(),
        ),
        (Some(p), _) => By::File(p.display().to_string()),
        (None, Source::Cli) => By::Env("GIT_CONFIG_PARAMETERS"),
        (None, _) => By::Env("GIT_CONFIG_COUNT"),
    };
    let (mut by_hook, mut by_event) = (None, None);
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
        let slot = if name == hook.as_bytes() {
            &mut by_hook
        } else if name == event.as_bytes() {
            &mut by_event
        } else {
            continue;
        };
        if let Some(on) = last {
            *slot = Some((on, section.meta()));
        }
    }
    let (key, meta) = match (by_hook, by_event) {
        (Some((false, meta)), _) => (hook, meta),
        (_, Some((false, meta))) => (event, meta),
        _ => return Switch::On,
    };
    Switch::Off {
        key: format!("hook.{key}.enabled"),
        by: by(meta),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_git_c_parameters_as_git_quotes_them() {
        let entry = |k: &str, v: Option<&str>| (k.to_string(), v.map(String::from));
        assert_eq!(
            parameters(r"'hook.a.enabled'='false' 'core.x'='it'\''s'"),
            Some(vec![
                entry("hook.a.enabled", Some("false")),
                entry("core.x", Some("it's"))
            ])
        );
        assert_eq!(
            parameters("'hook.b.enabled'= 'hook.c.enabled=no' 'hook.d.enabled'"),
            Some(vec![
                entry("hook.b.enabled", None),
                entry("hook.c.enabled", Some("no")),
                entry("hook.d.enabled", None)
            ])
        );
        assert_eq!(parameters(""), Some(vec![]));
        for bogus in ["'unclosed", "'k'=v", "'k''v'"] {
            assert_eq!(parameters(bogus), None, "{bogus}");
        }
    }

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
