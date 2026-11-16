use crate::staged;
use gix::{
    bstr::ByteSlice,
    config::{
        File, Source,
        file::Metadata,
        parse::{EventRef, Events},
    },
};
use std::{env, path::Path};

pub enum Switch {
    On,
    // The key that switches the hook off; "local" or "worktree" when the repo's
    // own config sets it; and the file that does, named as git names it.
    Off {
        key: String,
        scope: Option<&'static str>,
        by: String,
    },
    // Why git's answer can't be read here, with the fix where there is a safe one.
    Unclear(String),
}

pub fn git_bool(value: &str) -> Option<bool> {
    gix::config::Boolean::try_from(value).ok().map(bool::from)
}

// Whether git, in dir, runs hook for event or skips it. As git's hooks do, it
// reads every config scope but not GIT_CONFIG, the global ones alone outside a
// repo, and takes the last value that is a bool. gix's ceiling check is off by
// one against git's, so with ceiling directories set it could find a repo git
// doesn't; and gix opens a repo owned by someone else, and not in
// safe.directory, with its own config, where git reads no repo at all.
pub fn read(dir: &Path, hook: &str, event: &str) -> Switch {
    if env::var_os("GIT_CEILING_DIRECTORIES").is_some_and(|v| !v.is_empty()) {
        return Switch::Unclear(
            "as GIT_CEILING_DIRECTORIES is set; fix: run it without GIT_CEILING_DIRECTORIES".into(),
        );
    }
    let Ok(repo) = staged::open(dir) else {
        return File::from_globals().map_or(Switch::On, |config| rule(&config, None, hook, event));
    };
    if repo.git_dir_trust() != gix::sec::Trust::Full {
        return Switch::Unclear("as git doesn't trust this repo".into());
    }
    rule(
        repo.config_snapshot().plumbing(),
        repo.workdir(),
        hook,
        event,
    )
}

fn rule(config: &File, workdir: Option<&Path>, hook: &str, event: &str) -> Switch {
    // git shows the repo's own config by its path in the worktree.
    let by = |meta: &Metadata| match &meta.path {
        Some(p) if matches!(meta.source, Source::Local | Source::Worktree) => workdir
            .and_then(|w| p.strip_prefix(w).ok())
            .unwrap_or(p)
            .display()
            .to_string(),
        Some(p) => p.display().to_string(),
        None => "git's environment".into(),
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
        let slot = if name == hook {
            &mut by_hook
        } else if name == event {
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
        scope: match meta.source {
            Source::Local => Some("local"),
            Source::Worktree => Some("worktree"),
            _ => None,
        },
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
