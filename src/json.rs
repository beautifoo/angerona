use crate::{
    scan::{Item, Label, blob_item},
    staged,
    switch::{self, Switch},
};
use gix::{bstr::ByteSlice, remote::Direction};
use serde_json::Value;
use std::{collections::BTreeSet, fs, path::Path};

// What a call sends: each string, labelled by its path of keys and indices
// joined by ".", or "input" when the input is a string; the file a string
// names, as one to upload; then each key.
pub fn items(input: &Value, cwd: &Path) -> Vec<Item> {
    let (mut items, mut keys) = (Vec::new(), BTreeSet::new());
    walk(input, None, cwd, &mut items, &mut keys);
    items.extend(keys.into_iter().map(|k| Item {
        bytes: k.clone().into_bytes(),
        label: Label::Name("key", k),
    }));
    items
}

fn walk(
    v: &Value,
    path: Option<String>,
    cwd: &Path,
    items: &mut Vec<Item>,
    keys: &mut BTreeSet<String>,
) {
    let child = |k: &str| Some(path.as_ref().map_or(k.into(), |p| format!("{p}.{k}")));
    match v {
        Value::String(s) => {
            // A directory, or a path that doesn't exist yet, names nothing to read.
            let file = cwd.join(s);
            let named = (!s.contains('\n') && s.len() <= 4096 && file.is_file())
                .then(|| fs::read(&file).ok())
                .flatten();
            items.push(Item {
                label: Label::Blob(path.unwrap_or_else(|| "input".into())),
                bytes: s.clone().into_bytes(),
            });
            if let Some(bytes) = named {
                items.push(blob_item(s, &bytes));
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                walk(x, child(&i.to_string()), cwd, items, keys);
            }
        }
        Value::Object(m) => {
            for (k, x) in m {
                keys.insert(k.clone());
                walk(x, child(k), cwd, items, keys);
            }
        }
        _ => {}
    }
}

// Whether git, in dir, skips the push hook: then the session works on a repo
// that may name the terms, and this returns the GitHub owners of the remote the
// current branch pushes to. Outside a repo, there is none: no work session.
pub fn work_owners(dir: &Path) -> Option<Vec<String>> {
    let Switch::Off { .. } = switch::read(dir, "angerona-push", false) else {
        return None;
    };
    let repo = staged::open(dir).ok()?;
    let config = repo.config_snapshot();
    // The remote the branch pushes to, as git resolves it. A remote that doesn't
    // exist, or has no URL, names no owner.
    let branch = repo.head_name().ok()?.and_then(|n| {
        n.as_bstr()
            .strip_prefix(b"refs/heads/")
            .map(|b| b.to_str_lossy().into_owned())
    });
    let setting = |key: &str| config.string(key).map(|v| v.to_str_lossy().into_owned());
    let of_branch = |key: &str| {
        branch
            .as_ref()
            .and_then(|b| setting(&format!("branch.{b}.{key}")))
    };
    let remote = of_branch("pushRemote")
        .or_else(|| setting("remote.pushDefault"))
        .or_else(|| of_branch("remote"))
        .unwrap_or_else(|| "origin".into());
    Some(
        repo.find_remote(remote.as_str())
            .map(|r| {
                r.urls(Direction::Push)
                    .filter_map(|u| owner_of(&u.to_bstring().to_str_lossy()))
                    .collect()
            })
            .unwrap_or_default(),
    )
}

// The owner a GitHub URL names, lowercased. A GitHub Enterprise host counts
// by "github" in its name.
pub fn owner_of(url: &str) -> Option<String> {
    let url = gix::url::parse(url).ok()?;
    if !url.host()?.to_lowercase().contains("github") {
        return None;
    }
    let path = url.path.to_str_lossy();
    path.split('/')
        .find(|s| !s.is_empty())
        .map(str::to_lowercase)
}

// In a work session a GitHub call, a tool whose MCP server name has "github"
// in it in any case, passes only when it names an owner or organization, at
// any depth, and each one it names is one of the repo's. Every other tool
// passes.
pub fn passes(tool_name: &str, input: &Value, owners: &[String]) -> bool {
    let server = tool_name
        .strip_prefix("mcp__")
        .and_then(|rest| rest.rsplit_once("__"))
        .map_or("", |(server, _)| server);
    if !server.to_lowercase().contains("github") {
        return true;
    }
    let mut named = Vec::new();
    owners_named(input, &mut named);
    !named.is_empty()
        && named.iter().all(|v| {
            v.as_str()
                .is_some_and(|s| owners.contains(&s.to_lowercase()))
        })
}

fn owners_named<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if k == "owner" || k == "organization" {
                    out.push(x);
                }
                owners_named(x, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| owners_named(x, out)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn finds_github_owners_in_remote_urls() {
        assert_eq!(
            owner_of("git@github.com:WorkOrg/app.git").as_deref(),
            Some("workorg")
        );
        assert_eq!(
            owner_of("https://github.com/workorg/app").as_deref(),
            Some("workorg")
        );
        assert_eq!(
            owner_of("ssh://git@github.example.com/org/r.git").as_deref(),
            Some("org")
        );
        assert_eq!(owner_of("https://bitbucket.org/ws/app.git"), None);
        assert_eq!(owner_of("/tmp/origin.git"), None);
    }

    #[test]
    fn passes_github_calls_only_for_the_repos_owners() {
        let o = vec!["workorg".to_string()];
        assert!(passes(
            "mcp__github__create_issue",
            &json!({"owner": "WorkOrg", "body": "x"}),
            &o
        ));
        assert!(!passes(
            "mcp__github__create_issue",
            &json!({"owner": "me", "body": "x"}),
            &o
        ));
        assert!(!passes(
            "mcp__plugin_github_github__search_code",
            &json!({"query": "x"}),
            &o
        ));
        assert!(!passes(
            "mcp__github__fork_repository",
            &json!({"owner": "workorg", "organization": "me"}),
            &o
        ));
        for tool in [
            "mcp__github-work__create_issue",
            "mcp__GITHUB__create_issue",
        ] {
            assert!(
                !passes(tool, &json!({"owner": "me", "body": "x"}), &o),
                "{tool}"
            );
        }
        assert!(passes(
            "mcp__plugin_slack_slack__slack_send_message",
            &json!({"message": "x"}),
            &o
        ));
    }

    #[test]
    fn labels_strings_by_path_and_keys_by_name() {
        let it = items(&json!({"files": [{"content": "a"}]}), Path::new("/"));
        assert!(
            it.iter()
                .any(|i| matches!(&i.label, Label::Blob(p) if p == "files.0.content"))
        );
        assert!(
            it.iter()
                .any(|i| matches!(&i.label, Label::Name("key", k) if k == "content"))
        );
    }
}
