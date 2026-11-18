use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

const NEW_GIT: &str = "install git 2.54 or later, e.g. brew install git";

fn executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

// Checks that the git on PATH runs config hooks; prints each problem with the
// command that fixes it.
pub fn run() -> i32 {
    let mut problems = String::new();
    let mut notes = String::new();
    check_gits(&mut problems, &mut notes);

    if !notes.is_empty() {
        print!("angerona: note: {notes}");
    }
    if problems.is_empty() {
        println!("angerona: git is new enough");
        0
    } else {
        print!("angerona: {problems}");
        1
    }
}

// A git before 2.54, or one that can't run, skips config hooks without a
// word. The first git on PATH is the one that runs them here, so it adds to
// problems; another adds only to notes, since an app may still commit with
// it. Says whether the first one runs them.
fn check_gits(problems: &mut String, notes: &mut String) -> bool {
    let mut gits: Vec<PathBuf> = Vec::new();
    let path = env::var_os("PATH").unwrap_or_default();
    for dir in env::split_paths(&path).filter(|d| !d.as_os_str().is_empty()) {
        let g = dir.join("git");
        if executable(&g) && !gits.contains(&g) {
            gits.push(g);
        }
    }
    let Some(first) = gits.first() else {
        *problems += &format!("no git on PATH; fix: {NEW_GIT}\n");
        return false;
    };

    let mut good = None;
    let mut bad = Vec::new();
    for g in &gits {
        let version = version_line(g);
        if new_enough(&version) {
            if good.is_none() {
                good = Some(g);
            }
        } else {
            bad.push((g, version));
        }
    }

    let dir = |g: &Path| g.parent().unwrap_or(g).display().to_string();
    let mut first_runs_hooks = true;
    for (g, version) in bad {
        let fix = match good {
            None => NEW_GIT.to_string(),
            Some(good) if g == first => format!("put {} before {} on PATH", dir(good), dir(g)),
            Some(good) => format!("point any app that uses it at {}", good.display()),
        };
        let msg = format!(
            "{} ({version}) skips config hooks, so a commit or push made with it skips the guard; fix: {fix}\n",
            g.display()
        );
        if g == first {
            first_runs_hooks = false;
            *problems += &msg;
        } else {
            *notes += &msg;
        }
    }
    first_runs_hooks
}

// The first line git --version prints on stdout, past any warning, or on
// stderr when there is none; nothing when it can't run.
fn version_line(git: &Path) -> String {
    let Ok(out) = Command::new(git).arg("--version").output() else {
        return String::new();
    };
    let first_line = |b: &[u8]| {
        String::from_utf8_lossy(b)
            .lines()
            .next()
            .unwrap_or("")
            .to_string()
    };
    let line = first_line(&out.stdout);
    if line.is_empty() {
        first_line(&out.stderr)
    } else {
        line
    }
}

// Whether a "git version 2.54.0" line names 2.54 or later.
fn new_enough(version: &str) -> bool {
    let parts: Vec<&str> = version.split([' ', '.']).collect();
    let number = |i: usize| -> u32 {
        let part = parts.get(i).copied().unwrap_or("");
        let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().unwrap_or(0)
    };
    let major = number(2);
    let minor = number(3);
    major > 2 || (major == 2 && minor >= 54)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_git_version_as_2_54_or_later() {
        for v in [
            "git version 2.54.0",
            "git version 2.56.1",
            "git version 3.0.0",
            "git version 2.54.0.windows.1",
        ] {
            assert!(new_enough(v), "{v}");
        }
        for v in [
            "git version 2.53.0",
            "git version 2.39.5 (Apple Git-154)",
            "warning: x",
            "",
        ] {
            assert!(!new_enough(v), "{v}");
        }
    }
}
