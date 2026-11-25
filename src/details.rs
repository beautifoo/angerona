use crate::verdict::{EmailFacts, Outside};
use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

// Logins and host names too common to mean you; matching them would flag
// strangers' text.
const GENERIC_LOGINS: &[&str] = &[
    "root",
    "admin",
    "administrator",
    "user",
    "dev",
    "test",
    "pi",
    "ubuntu",
    "debian",
    "runner",
    "vagrant",
    "ec2-user",
    "git",
    "guest",
];
const GENERIC_HOSTS: &[&str] = &[
    "mac",
    "macbook",
    "macbook-pro",
    "macbook-air",
    "imac",
    "mac-mini",
    "ubuntu",
    "debian",
    "raspberrypi",
    "linux",
    "localhost",
    "computer",
];

#[cfg(feature = "github-emails")]
const SETUP_WHERE: &str = "GitHub account (angerona setup)";

// What this machine says about you, read raw; discover turns it into Details.
pub struct Machine {
    pub home: Option<PathBuf>,
    pub login: Option<String>,
    pub hosts: Vec<String>,
    pub env_emails: Vec<String>,
    pub config_files: Vec<PathBuf>,
    #[cfg(feature = "github-emails")]
    pub emails_file: Option<PathBuf>,
}

// What counts as you, ready to build a matcher from.
pub struct Details {
    pub emails: Vec<(String, String)>,
    pub home: Option<String>,
    pub raw_login: Option<String>,
    pub login: Option<String>,
    pub hosts: Vec<String>,
    pub weak: Vec<String>,
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

// $XDG_CONFIG_HOME when absolute, else ~/.config, as git does.
fn config_home() -> Option<PathBuf> {
    match env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
        Some(x) if x.is_absolute() => Some(x),
        _ => home_dir().map(|h| h.join(".config")),
    }
}

// Where setup keeps the addresses it learned from your GitHub account.
#[cfg(feature = "github-emails")]
pub fn emails_path() -> Option<PathBuf> {
    config_home().map(|c| c.join("angerona").join("emails"))
}

fn command_line(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    let line = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !line.is_empty()).then_some(line)
}

// Reads the machine's home, login, host names, emails and git config paths.
// ComputerName is never read: it is often a plain name or phrase.
pub fn machine() -> Machine {
    let home = home_dir().and_then(|h| fs::canonicalize(h).ok());
    let login = env::var("USER")
        .ok()
        .filter(|l| !l.is_empty())
        .or_else(|| env::var("LOGNAME").ok().filter(|l| !l.is_empty()))
        .or_else(|| {
            let name = home.as_ref()?.file_name()?;
            Some(name.to_string_lossy().into_owned())
        });

    let test_hosts = env::var("ANGERONA_TEST_HOSTS").ok();
    let hosts: Vec<String> = match test_hosts {
        Some(t) if cfg!(debug_assertions) => t
            .split(',')
            .filter(|h| !h.is_empty())
            .map(str::to_string)
            .collect(),
        _ => {
            // The name hostname prints, read with no program: Arch base and
            // Fedora ship no hostname.
            let node = rustix::system::uname()
                .nodename()
                .to_string_lossy()
                .into_owned();
            let mut hosts: Vec<String> = Some(node).filter(|n| !n.is_empty()).into_iter().collect();
            if cfg!(target_os = "macos") {
                for key in ["HostName", "LocalHostName"] {
                    hosts.extend(command_line("scutil", &["--get", key]));
                }
            }
            hosts
        }
    };

    let env_emails = ["EMAIL", "GIT_AUTHOR_EMAIL", "GIT_COMMITTER_EMAIL"]
        .iter()
        .filter_map(|k| env::var(k).ok())
        .filter(|e| !e.is_empty())
        .collect();

    let mut config_files = Vec::new();
    if env::var_os("GIT_CONFIG_NOSYSTEM").is_none() {
        let system =
            env::var_os("GIT_CONFIG_SYSTEM").map_or(PathBuf::from("/etc/gitconfig"), PathBuf::from);
        config_files.push(system);
    }
    if let Some(global) = env::var_os("GIT_CONFIG_GLOBAL") {
        config_files.push(PathBuf::from(global));
    } else {
        config_files.extend(config_home().map(|c| c.join("git").join("config")));
        config_files.extend(home_dir().map(|h| h.join(".gitconfig")));
    }

    Machine {
        home,
        login,
        hosts,
        env_emails,
        config_files,
        #[cfg(feature = "github-emails")]
        emails_file: emails_path().filter(|p| p.exists()),
    }
}

// Noreply addresses carry nothing personal, so they are not worth hiding.
pub fn is_noreply(email: &str) -> bool {
    let email = email.to_lowercase();
    let (local, domain) = email.rsplit_once('@').unwrap_or((&email, ""));
    local == "noreply"
        || local == "no-reply"
        || domain.starts_with("noreply.")
        || domain.starts_with("users.noreply.")
}

// Lowercases a host and cuts it to its first label; None when what is left
// is too short or too common to mean you.
pub fn normalize_host(h: &str) -> Option<String> {
    let h = h.trim().to_lowercase();
    let h = h.split('.').next().unwrap_or(&h);
    (h.chars().count() >= 4 && !GENERIC_HOSTS.contains(&h)).then(|| h.to_string())
}

// A login this short or common would match strangers' text, whatever its case.
pub fn weak_login(l: &str) -> bool {
    l.chars().count() < 3 || GENERIC_LOGINS.contains(&l.to_lowercase().as_str())
}

fn expand_tilde(p: &str) -> PathBuf {
    match (p.strip_prefix("~/"), home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(p),
    }
}

// Every value of the key in the files and in everything they include, with
// the file it is in, whatever the includeIf condition: a user.email set under
// any of them may be one you commit with. The visited set ends include loops.
pub fn config_values(files: &[PathBuf], key: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut visited = HashSet::new();
    let mut todo: Vec<PathBuf> = files.iter().rev().cloned().collect();
    while let Some(path) = todo.pop() {
        let real = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if !visited.insert(real) {
            continue;
        }
        let Ok(config) =
            gix::config::File::from_path_no_includes(path.clone(), gix::config::Source::User)
        else {
            continue;
        };
        for value in config.strings(key).unwrap_or_default() {
            found.push((value.to_string(), path.display().to_string()));
        }
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut includes = Vec::new();
        for name in ["include", "includeIf"] {
            for section in config.sections_by_name(name).into_iter().flatten() {
                for value in section.values("path") {
                    includes.push(dir.join(expand_tilde(&value.to_string())));
                }
            }
        }
        todo.extend(includes.into_iter().rev());
    }
    found
}

// Your emails and raw login from d, and the user.email git uses in the repo it
// runs in, with every includeIf resolved there, and the one it uses outside
// any repo: from / with no GIT_DIR, so every config file and plain include
// counts and no includeIf on a gitdir or branch matches. The verdict decides.
pub fn email_facts(d: &Details) -> EmailFacts {
    let outside = Command::new("git")
        .args(["config", "--get", "user.email"])
        .current_dir("/")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .ok()
        .and_then(|out| {
            let line = String::from_utf8(out.stdout).ok()?.trim().to_lowercase();
            match out.status.code() {
                Some(0) if !line.is_empty() => Some(Outside::Set(line)),
                Some(1) if line.is_empty() => Some(Outside::Unset),
                _ => None,
            }
        })
        .unwrap_or(Outside::GitFailed);
    EmailFacts {
        own: d.emails.iter().map(|(e, _)| e.clone()).collect(),
        repo_email: command_line("git", &["config", "--get", "user.email"])
            .map(|e| e.to_lowercase()),
        outside,
        login: d.raw_login.clone(),
    }
}

// Text shaped like an address: something, an @, then something.
pub fn is_address(text: &str) -> bool {
    text.rsplit_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && !domain.is_empty())
}

// Gathers every email, the home path, login and host names, and says in weak
// which login or host was left out for being too common.
pub fn discover(m: &Machine) -> Details {
    let mut found: Vec<(String, String)> = m
        .env_emails
        .iter()
        .map(|e| (e.clone(), "environment".to_string()))
        .collect();
    found.extend(config_values(&m.config_files, "user.email"));
    #[cfg(feature = "github-emails")]
    if let Some(text) = m
        .emails_file
        .as_ref()
        .and_then(|f| fs::read_to_string(f).ok())
    {
        for line in text.lines().map(str::trim) {
            if !line.is_empty() && !line.starts_with('#') {
                found.push((line.to_string(), SETUP_WHERE.to_string()));
            }
        }
    }
    let mut emails: Vec<(String, String)> = Vec::new();
    for (email, place) in found {
        let email = email.trim().to_lowercase();
        if !email.is_empty() && !is_noreply(&email) && !emails.iter().any(|(e, _)| *e == email) {
            emails.push((email, place));
        }
    }

    let mut weak = Vec::new();
    let login = match &m.login {
        Some(l) if weak_login(l) => {
            weak.push(format!("login {l} is too common to match"));
            None
        }
        other => other.clone(),
    };
    let mut hosts: Vec<String> = Vec::new();
    for h in &m.hosts {
        match normalize_host(h) {
            Some(n) if !hosts.contains(&n) => hosts.push(n),
            Some(_) => {}
            None => {
                // Hostname, HostName and LocalHostName often differ only in case.
                let first = h.trim().to_lowercase();
                let line = format!(
                    "host {} is too short or common to match",
                    first.split('.').next().unwrap_or(&first)
                );
                if !weak.contains(&line) {
                    weak.push(line);
                }
            }
        }
    }

    Details {
        emails,
        home: m.home.as_ref().map(|h| h.display().to_string()),
        raw_login: m.login.clone(),
        login,
        hosts,
        weak,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // A fresh temp dir for one test; the test removes it at the end.
    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("angerona-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn machine_with_configs(files: Vec<PathBuf>) -> Machine {
        Machine {
            home: None,
            login: None,
            hosts: vec![],
            env_emails: vec![],
            config_files: files,
            #[cfg(feature = "github-emails")]
            emails_file: None,
        }
    }

    #[test]
    fn noreply_addresses() {
        for e in [
            "123+me@users.noreply.github.com",
            "me@users.noreply.gitlab.example",
            "me@noreply.codeberg.org",
            "noreply@anthropic.com",
            "NO-REPLY@x.io",
        ] {
            assert!(is_noreply(e), "{e}");
        }
        for e in ["x.noreply@gmail.com", "me@example.com", "noreplyfan@x.io"] {
            assert!(!is_noreply(e), "{e}");
        }
    }

    #[test]
    fn follows_includes_whatever_the_condition() {
        let dir = temp("includes");
        let global = dir.join("global");
        let work = dir.join("work.inc");
        let nested = dir.join("nested.inc");
        fs::write(
            &global,
            "[user]\n\temail = 1+me@users.noreply.github.com\n[includeIf \"gitdir:~/work/\"]\n\tpath = work.inc\n",
        )
        .unwrap();
        fs::write(
            &work,
            "[user]\n\temail = Me@Work.Example\n[include]\n\tpath = nested.inc\n",
        )
        .unwrap();
        fs::write(&nested, "[user]\n\temail = other@corp.example\n").unwrap();
        let d = discover(&machine_with_configs(vec![global]));
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            d.emails,
            vec![
                ("me@work.example".to_string(), work.display().to_string()),
                (
                    "other@corp.example".to_string(),
                    nested.display().to_string()
                ),
            ]
        );
    }

    #[test]
    fn include_loops_end() {
        let dir = temp("loops");
        let a = dir.join("a");
        let b = dir.join("b");
        fs::write(&a, "[user]\n\temail = a@x.example\n[include]\n\tpath = b\n").unwrap();
        fs::write(&b, "[user]\n\temail = b@x.example\n[include]\n\tpath = a\n").unwrap();
        let found = config_values(&[a], "user.email");
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn env_emails_count() {
        let mut m = machine_with_configs(vec![]);
        m.env_emails = vec!["Mine@Home.Example".to_string()];
        let d = discover(&m);
        assert_eq!(d.emails[0].0, "mine@home.example");
    }

    #[cfg(feature = "github-emails")]
    #[test]
    fn emails_file_counts() {
        let dir = temp("emails-file");
        let file = dir.join("emails");
        fs::write(&file, "# from GitHub\nMe@Personal.Example\n\n").unwrap();
        let mut m = machine_with_configs(vec![]);
        m.emails_file = Some(file);
        let d = discover(&m);
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            d.emails,
            vec![(
                "me@personal.example".to_string(),
                "GitHub account (angerona setup)".to_string()
            )]
        );
    }

    #[test]
    fn hosts_are_normalized_and_floored() {
        assert_eq!(
            normalize_host("Jdoes-Laptop.local"),
            Some("jdoes-laptop".to_string())
        );
        assert_eq!(normalize_host("box.lan"), None);
        assert_eq!(normalize_host("MacBook-Pro"), None);
        assert_eq!(normalize_host("localhost"), None);
    }

    #[test]
    fn weak_logins_are_left_out() {
        let mut m = machine_with_configs(vec![]);
        m.login = Some("dev".to_string());
        let d = discover(&m);
        assert!(d.login.is_none() && d.weak.len() == 1 && d.weak[0].contains("dev"));
        m.login = Some("jdoe".to_string());
        assert_eq!(discover(&m).login, Some("jdoe".to_string()));
    }

    #[test]
    fn weak_lines_are_not_repeated_and_the_floor_ignores_case() {
        let mut m = machine_with_configs(vec![]);
        m.login = Some("Root".to_string());
        m.hosts = vec![
            "MacBook-Pro.local".to_string(),
            "macbook-pro".to_string(),
            "MACBOOK-PRO".to_string(),
        ];
        let d = discover(&m);
        assert_eq!(d.weak.len(), 2);
    }
    #[test]
    fn discovers_nothing_without_home_or_config() {
        let d = discover(&machine_with_configs(vec![]));
        assert!(
            d.emails.is_empty()
                && d.home.is_none()
                && d.raw_login.is_none()
                && d.login.is_none()
                && d.hosts.is_empty()
                && d.weak.is_empty()
        );
    }
}
