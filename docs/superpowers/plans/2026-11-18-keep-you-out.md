# Keep You Out Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn angerona from a private-term guard into a tool that keeps your personal details (emails, home path, login, machine name) out of what you push to public repos, with `setup`, a pre-push hook, `scan` and `doctor`.

**Architecture:** One Rust binary on top of the current `main` (approach A). git runs `angerona check-push` as a git ≥ 2.54 config-based pre-push hook. It decides with one credential-free HTTP GET whether the push URL is public, walks the outgoing commits in-process with gix, computes each commit's added lines by set difference against its parents, and matches them, the paths, binaries, identities, trailers, messages and the remote ref name against the personal details found on the machine.

**Tech Stack:** Rust 2024 (rust-version 1.88), gix 0.90 (features as today), `regex` 1, `curl` 0.4 (already in the tree through gix's transport), bats for end-to-end tests, cargo-dist 0.34.

**Spec:** `docs/superpowers/specs/2026-11-18-keep-you-out-design.md`. Vocabulary: `GLOSSARY.md`. Read both before any task.

## Global Constraints

- Work on branch `feat/keep-you-out`. Every commit builds and passes: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo build && bats test/`.
- Commits are signed (`git commit -S`, check `git log -1 --format=%G?` prints `G`), Sentry-style subjects as in `git log` (`feat: …`, `ref: …`, `test: …`, `docs: …`, `ci: …`), body lines under 100 characters, **no** `Co-Authored-By`, `Claude-Session` or other attribution lines.
- Never `git commit --no-verify` or `git push --no-verify`: the repo's gitleaks hooks must run. Never stage `HANDOFF.md`. Never push, tag, or change the GitHub repo without the owner's yes.
- Never read `~/.config/gitleaks/work.toml` or `~/.claude/settings.json`.
- No new crates. `aho-corasick` and `serde_json` go away in Task 1; `curl` moves from the Linux-only table to `[dependencies]` in Task 5.
- macOS and Linux only. git ≥ 2.54.
- Code reads like the surrounding code: a short comment above each function saying why, not what; plain `for` loops and `let`s over iterator chains where the existing code does so; errors as `Box<dyn Error>` where git is read; `ponytail:` comments for a known ceiling.
- Fail closed: any error while reading the repo or the push exits 1 with `angerona: <reason>, refusing to push` (`main.rs`'s `fail`). The single fail-open is the missing binary (Task 9's hook line).
- Test seams are debug-only (`#[cfg(debug_assertions)]`): `ANGERONA_TEST_VISIBILITY` (`public`, `private`, `unprobeable`, `passing`) in `public::probe`, `ANGERONA_TEST_HOSTS` (comma-separated) in `details::machine`. bats runs the debug binary; CI changes accordingly in Task 5.
- Exact user-facing strings fixed by the spec:
  - `Treat <url> as public? [Y/n]`
  - `Push anyway? [y/N]`
  - `angerona not installed, push not checked`
  - `repos already public are not rechecked; run angerona scan in each`
  - `run: gh auth refresh -h github.com -s user:email, then angerona setup again`
  - `gh not available: only the emails in your git config are known`

## Review Focus

- **Bytes that aren't UTF-8** (Latin-1 messages, binary-ish text files): no panic, matches still found around them. Test in Task 3 (`matches_around_invalid_utf8`) and Task 7 (bats `a Latin-1 message still matches`).
- **A push of only a tag, or only deletions**: an annotated tag on an already-pushed commit is still read; a deletion-only push passes without probing. Task 7 bats tests `a tag on a pushed commit is checked` and `a deletion-only push passes`.
- **URLs carrying credentials or odd forms**: credentials never printed. Task 5 tests `https_form_drops_credentials`.
- **No identity and no HOME**: `details::discover` with `HOME` unset and no `user.email` returns empty fields, never panics; `doctor` reports it. Task 2 test `discovers_nothing_without_home_or_config` and Task 11 bats `doctor with no identity says how to set one`.
- **Linked worktrees and repos opened from a subdirectory**: the hook reads the same repo and config as git. Task 7 bats `the hook works from a linked worktree`.

---

### Task 1: Retire the old guard

Remove everything that belongs to the term-list scope so later tasks start from the reusable core.

**Files:**
- Delete: `src/list.rs`, `src/json.rs`, `src/switch.rs`, `src/msg.rs`, `src/staged.rs`
- Rename: `src/scan.rs` → `src/text.rs` (keep `Label`, `Item`, `blob_item`, `name_item`; delete `hits` and its tests; keep the blob tests, with `globex` replaced by any neutral word)
- Modify: `src/main.rs` (only `doctor` left, printing `angerona: being rebuilt, see README.md` and exiting 0; usage line `usage: angerona check-push REMOTE URL | scan | doctor | setup` with exit 2 for anything else)
- Modify: `src/doctor.rs` (keep only `executable`, `git`, `check_gits`, `version_line`, `new_enough` and its test; `run` calls `check_gits` and prints problems/notes as today)
- Modify: `src/push.rs` (drop `items`, `remote_files`; keep `RefLine`, `parse_ref_line`, `strip_signature`, `may_ask`, `held`, `list_refs`, `without_credentials`, `read`, `parents`, `remote_commits` and their tests; `#[allow(dead_code)]` on the module in `main.rs` until Task 7 uses it)
- Modify: `Cargo.toml` (remove `aho-corasick`, `serde_json`; `description = "Keeps your emails, home path, login and machine name out of what you publish with git"`)
- Modify: `test/angerona.bats` (replace every test with the `setup`/`teardown`/`has`/`lacks` helpers kept and one test: `usage on an unknown command` exits 2 and prints `usage:`; drop the term-list setup lines)
- Modify: `.github/workflows/test.yml` (drop the "release build speaks https" step; Task 5 adds its replacement)
- Modify: `docs/adr/0001-one-hook-name-per-event.md` (add front matter `Status: superseded`, one line: "The new scope runs one pre-push hook; see the 2026-11-18 design.")
- Modify: `README.md` (replace with: title, the Angerona line, one paragraph of the new scope from the spec's Intent, and "Not usable yet: being rebuilt on branch feat/keep-you-out.")

- [ ] **Step 1:** Make the deletions and edits above.
- [ ] **Step 2:** Run the global check command. Expected: all green; `cargo test` runs the kept push, text and doctor tests; bats runs 1 test.
- [ ] **Step 3:** Commit: `ref!: Retire the term-list guard` with a body naming what moved to gitleaks.

---

### Task 2: Find the personal details on the machine

**Files:**
- Create: `src/details.rs`
- Modify: `src/main.rs` (`mod details;`)

**Interfaces:**
- Produces:
  - `pub struct Machine { pub home: Option<PathBuf>, pub login: Option<String>, pub hosts: Vec<String>, pub env_emails: Vec<String>, pub config_files: Vec<PathBuf>, pub emails_file: Option<PathBuf> }`; `emails_file` = `emails_path()` when it exists.
  - `pub fn emails_path() -> Option<PathBuf>`: `$XDG_CONFIG_HOME/angerona/emails` when `XDG_CONFIG_HOME` is absolute, else `~/.config/angerona/emails` (Task 9 writes it). `discover` reads it one address per line (blank lines and `#` lines skipped), "where" = `GitHub account (angerona setup)`.
  - `pub fn machine() -> Machine`: `home` = `realpath($HOME)`; `login` = `$USER`, else `$LOGNAME`, else `home`'s last component; `hosts` = output of `hostname`, plus on macOS `scutil --get HostName` and `scutil --get LocalHostName` (each trimmed, failures skipped; ComputerName never read); debug builds take `ANGERONA_TEST_HOSTS` instead when set; `env_emails` = `EMAIL`, `GIT_AUTHOR_EMAIL`, `GIT_COMMITTER_EMAIL` when set; `config_files` = `GIT_CONFIG_SYSTEM` or `/etc/gitconfig` (skipped when `GIT_CONFIG_NOSYSTEM` is set), `GIT_CONFIG_GLOBAL` when set, else `$XDG_CONFIG_HOME/git/config` (or `~/.config/git/config`) and `~/.gitconfig`.
  - `pub struct Details { pub emails: Vec<(String, String)>, pub home: Option<String>, pub raw_login: Option<String>, pub login: Option<String>, pub hosts: Vec<String>, pub weak: Vec<String> }`: `emails` are `(address, where found)`, lowercased, noreply ones dropped, deduplicated; `home` as a string; `raw_login` is the login as found (the home-path rule uses it even when weak); `login` is `None` when weak; `hosts` normalized; `weak` holds one human line per login or host left out by the floor.
  - `pub fn discover(m: &Machine) -> Details`
  - `pub fn config_emails(files: &[PathBuf]) -> Vec<(String, String)>`: every `user.email` in each file and in every file it reaches through `include.path` and `includeIf.*.path`, **whatever the condition**, following nested includes, a relative path resolved from the including file's directory, `~/` expanded; missing files skipped; each loop guarded by a visited set.
  - `pub fn is_noreply(email: &str) -> bool`: local part `noreply` or `no-reply`, or domain starting with `noreply.` or `users.noreply.` (case-insensitive).
  - `pub fn normalize_host(h: &str) -> Option<String>`: lowercase, strip a trailing `.local` and anything after the first dot; `None` when shorter than 4 characters, `localhost`, or in `GENERIC_HOSTS`.
  - `pub fn weak_login(l: &str) -> bool`: shorter than 3 characters or in `GENERIC_LOGINS`.
  - `const GENERIC_LOGINS: &[&str] = &["root", "admin", "administrator", "user", "dev", "test", "pi", "ubuntu", "debian", "runner", "vagrant", "ec2-user", "git", "guest"];`
  - `const GENERIC_HOSTS: &[&str] = &["mac", "macbook", "macbook-pro", "macbook-air", "imac", "mac-mini", "ubuntu", "debian", "raspberrypi", "linux", "localhost", "computer"];`

Read config files with `gix::config::File::from_path_no_includes(path, gix::config::Source::User)`, iterate `sections_by_name("include")` and `sections_by_name("includeIf")` for `path` values, and `strings("user.email")`.

- [ ] **Step 1: Write the failing tests** in `src/details.rs`:
  - `noreply_addresses`: `is_noreply` true for `123+me@users.noreply.github.com`, `me@users.noreply.gitlab.example`, `me@noreply.codeberg.org`, `noreply@anthropic.com`, `NO-REPLY@x.io`; false for `x.noreply@gmail.com`, `me@example.com`, `noreplyfan@x.io`.
  - `follows_includes_whatever_the_condition`: in a temp dir, `global` holds `[user] email = 1+me@users.noreply.github.com` and `[includeIf "gitdir:~/work/"] path = work.inc`; `work.inc` holds `[user] email = Me@Work.Example` and `[include] path = nested.inc`; `nested.inc` holds `[user] email = other@corp.example`. `discover` with `config_files = [global]` gives emails `me@work.example` and `other@corp.example`, each with its file path as "where".
  - `include_loops_end`: two files including each other give each email once.
  - `env_emails_count`: `env_emails = ["Mine@Home.Example"]` gives `mine@home.example`.
  - `emails_file_counts`: a file with `# from GitHub\nMe@Personal.Example\n\n` gives `me@personal.example` with where `GitHub account (angerona setup)`.
  - `hosts_are_normalized_and_floored`: `normalize_host("Jdoes-Laptop.local") == Some("jdoes-laptop")`, `normalize_host("box.lan") == None` (3 chars), `normalize_host("MacBook-Pro") == None`, `normalize_host("localhost") == None`.
  - `weak_logins_are_left_out`: `discover` with `login = Some("dev")` gives `login == None` and one `weak` line containing `dev`; with `"jdoe"` gives `Some("jdoe")`.
  - `discovers_nothing_without_home_or_config`: `Machine` with every field empty gives empty `Details`, no panic.
  Temp dirs: `std::env::temp_dir().join(format!("angerona-{}-{name}", std::process::id()))`, removed at the end of each test.
- [ ] **Step 2:** `cargo test details` fails (functions missing).
- [ ] **Step 3:** Implement the interfaces above in `src/details.rs`.
- [ ] **Step 4:** `cargo test details` passes; run the global check.
- [ ] **Step 5:** Commit `feat: Find the personal details on the machine`.

---

### Task 3: Match personal details in text

**Files:**
- Create: `src/matcher.rs`
- Modify: `src/main.rs` (`mod matcher;`)

**Interfaces:**
- Consumes: `details::{Details, is_noreply}`.
- Produces:
  - `#[derive(Debug, PartialEq)] pub enum Kind { Email, HomePath, Login, Host }`
  - `#[derive(Debug, PartialEq)] pub struct Hit { pub line: usize, pub kind: Kind, pub text: String }` (`text` is the matched span as found)
  - `pub struct Matcher` with `pub fn new(d: &Details) -> Matcher`, `pub fn find(&self, text: &str) -> Vec<Hit>` (by position, each span once) and `pub fn find_ref(&self, name: &str) -> Vec<Hit>` (`find` plus the branch-prefix rule).

Rules, all case-insensitive, built as one `regex::Regex` per kind with the details escaped (`regex::escape`):
- **Email:** every address in `d.emails`, as a whole address (not inside a longer local part or domain). Before any other rule, find every email-shaped span (`[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+`); spans that are noreply are masked out for the Login and Host rules.
- **HomePath:** `d.home` literally; and `(?:Users|home)(?:/|\\{1,2}|%2F)+<login>(?:[^A-Za-z0-9._-]|$)`; and `~<login>/`, with `<login>` = `d.raw_login` (the floor does not apply to the home path).
- **Login** (skipped when `d.login` is `None`): `<login>\.xcuserdatad`; `<login>@<host>` for each host in `d.hosts`, with or without a domain after the host.
- **Host:** `@<host>(?:\.[A-Za-z0-9-]+)*` as the full first domain label after `@`, not inside a noreply span.
- **find_ref only:** a ref name whose first component after `refs/heads/` or `refs/tags/` (or the bare name) equals the login is a `Login` hit.

- [ ] **Step 1: Write the failing tests** in `src/matcher.rs` with `Details { emails: [("me@home.example", "test")], home: Some("/Users/jdoe"), login: Some("jdoe"), hosts: ["jdoes-laptop"], .. }`:
  - `finds_home_path_in_every_form`: each of `/Users/jdoe/x`, `/USERS/JDOE/x`, `\/Users\/jdoe\/x`, `C:\\Users\\jdoe\\x`, `%2FUsers%2Fjdoe%2Fx`, `file:///users/Jdoe/x`, `~jdoe/x`, `/home/jdoe/x` gives one `HomePath` hit; `/Users/jdoeni/x` gives none.
  - `login_only_after_a_home_root`: `/dev/null`, `jdoe wrote this`, `src/jdoe/mod.rs` give no hit; `App.xcodeproj/xcuserdata/jdoe.xcuserdatad/x` gives a `Login` hit.
  - `prompt_text_is_found`: `jdoe@jdoes-laptop angerona %` gives one `Login` hit.
  - `host_only_as_the_label_after_at`: `x@jdoes-laptop.local` gives a `Host` hit; `https://jdoes-laptop.example.com` and `security@ubuntu.com` give none.
  - `own_email_whole_only`: `Me@Home.Example` hits `Email`; `notme@home.example` and `me@home.example.org` do not.
  - `noreply_is_never_a_hit`: `123+jdoe@users.noreply.github.com` gives no hit.
  - `ref_prefix_is_a_login`: `find_ref("refs/heads/jdoe/fix")` gives a `Login` hit; `find_ref("refs/heads/fix/jdoe-ui")` gives none.
  - `lines_are_counted`: `"a\nb\n/Users/jdoe\n"` gives a hit on line 3.
  - `matches_around_invalid_utf8`: `find(&String::from_utf8_lossy(b"caf\xe9\n/Users/jdoe\xff\n"))` gives one hit on line 2.
- [ ] **Step 2:** `cargo test matcher` fails.
- [ ] **Step 3:** Implement `Matcher`.
- [ ] **Step 4:** `cargo test matcher` passes; global check.
- [ ] **Step 5:** Commit `feat: Match personal details in text`.

---

### Task 4: Read identities, trailers and messages from raw objects

**Files:**
- Create: `src/identity.rs`
- Modify: `src/main.rs` (`mod identity;`)

**Interfaces:**
- Consumes: `push::strip_signature`, `details::is_noreply`.
- Produces:
  - `pub struct Ident { pub field: String, pub email: String, pub raw: String }` (`field` is `author`, `committer`, `tagger`, or the trailer key as written; `raw` is the whole `Name <email>` text)
  - `pub struct Object { pub idents: Vec<Ident>, pub message: Vec<u8> }`
  - `pub fn read(raw: &[u8]) -> Object`: headers up to the first blank line give `author`/`committer`/`tagger`; the message is the rest, signature stripped (`strip_signature` for commits; for tags, cut at `-----BEGIN PGP SIGNATURE-----`, `-----BEGIN SSH SIGNATURE-----` or `-----BEGIN SIGNED MESSAGE-----`); every message line matching `^([A-Za-z][A-Za-z0-9-]*): (.*<([^>]+)>)\s*$` is a trailer ident. Raw bytes only, never mailmap.
  - `pub fn foreign<'a>(o: &'a Object, unpublished: bool, own: &[String]) -> Vec<&'a Ident>`: idents whose field is `tagger`, or `committer` when `unpublished`, with an email that is not noreply and not in `own` (lowercased compare).

- [ ] **Step 1: Write the failing tests:**
  - `reads_headers_and_trailers`: a commit with `author A <a@x.example> 1 +0000`, `committer C <c@x.example> 1 +0000`, message `fix\n\nCo-authored-by: B <b@x.example>\nSigned-off-by: D <d@x.example>\n` gives fields `author`, `committer`, `Co-authored-by`, `Signed-off-by` with those emails.
  - `the_message_has_no_signature`: a commit with a `gpgsig` header gives a message without base64 lines; a tag with `-----BEGIN SSH SIGNATURE-----` below its message gives the message only.
  - `others_only_as_committer_of_unpublished_or_tagger`: for a commit with author `bob@corp.example` and committer `me@home.example`, `foreign(.., true, ["me@home.example"])` is empty; with committer `bob@corp.example`, `foreign(.., true, ..)` has one ident and `foreign(.., false, ..)` none; a tag with tagger `bob@corp.example` gives one either way; noreply committers give none.
- [ ] **Step 2:** `cargo test identity` fails.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Passes; global check.
- [ ] **Step 5:** Commit `feat: Read identities and trailers as stored`.

---

### Task 5: Tell whether a push URL is public

**Files:**
- Create: `src/public.rs`
- Modify: `src/main.rs` (`mod public;`), `Cargo.toml` (move `curl` to `[dependencies]` as `curl = { version = "0.4", default-features = false }`, keep the Linux table adding `features = ["ssl", "static-curl", "static-ssl"]`), `.github/workflows/test.yml`

**Interfaces:**
- Produces:
  - `#[derive(Debug, PartialEq)] pub enum Visibility { Public(Vec<gix::ObjectId>), Private, Unprobeable, Passing }`
  - `pub fn is_local(url: &str) -> bool`: a path, `file://` URL, or anything without `://` and without a `host:` scp-like prefix.
  - `pub fn https_form(url: &str, resolve: &dyn Fn(&str) -> Option<String>) -> Option<String>`: `https://` kept, user and password dropped (`push::without_credentials`, made `pub`); `http://` kept; `ssh://[user@]host[:port]/path` and scp-like `[user@]host:path` become `https://<resolved host>/<path>` with `.git` kept as given; `resolve(host)` returns the `hostname` line of `ssh -G <host>` (stdin closed); `ssh.github.com` becomes `github.com`; `git://` becomes `https://`; anything else `None`.
  - `pub enum Answer { Http { status: u32, content_type: String, body: Vec<u8> }, Timeout, Dns, Refused, Tls, Other }`
  - `pub fn classify(a: Answer) -> Visibility`: final `200` with `application/x-git-upload-pack-advertisement` → `Public(advertised(body))`; `401 | 403 | 404` → `Private`; `Refused`, `Tls`, any other `200`, any `2xx`/`3xx` left after redirects → `Unprobeable`; `Timeout`, `Dns`, `5xx`, `429`, `Other` → `Passing`.
  - `pub fn advertised(body: &[u8]) -> Vec<gix::ObjectId>`: pkt-lines (4 hex digits of length, `0000` flush), skipping the `# service=` line; each ref line's first 40 (or 64) hex chars, `^{}` peeled lines included.
  - `pub fn probe(url: &str) -> Visibility`: debug builds check `ANGERONA_TEST_VISIBILITY` first (`public` gives `Public(vec![])`), so bats can push to local bare repos; then `is_local` → `Private`; no `https_form` → `Unprobeable`; else one GET of `<https>/info/refs?service=git-upload-pack` with the `curl` crate: `follow_location(true)`, `connect_timeout(3s)`, `timeout(5s)`, `netrc(NetRc::Ignored)`, no auth, no extra headers, mapping curl errors to `Answer` (`is_operation_timedout` → Timeout, `is_couldnt_resolve_host` → Dns, `is_couldnt_connect` → Refused, `is_ssl_*`/`is_peer_failed_verification` → Tls).

- [ ] **Step 1: Write the failing tests:**
  - `https_form_maps_ssh`: with `resolve = |h| if h == "github-work" { Some("github.com") } else { Some(h) }`: `git@github-work:o/r.git` → `https://github.com/o/r.git`; `ssh://git@ssh.github.com:443/o/r.git` → `https://github.com/o/r.git`; `ssh://git@gitea.example:2222/o/r` → `https://gitea.example/o/r`; `git://host/r.git` → `https://host/r.git`.
  - `https_form_drops_credentials`: `https://me:tok@github.com/o/r.git` → `https://github.com/o/r.git`.
  - `local_urls_are_private`: `is_local` true for `/srv/r.git`, `../r.git`, `file:///srv/r.git`; false for `git@github.com:o/r.git`.
  - `classifies_answers`: one assertion per row of `classify`'s table above, including `Http{200,"text/html"}` → `Unprobeable` and `Http{203,"text/html"}` → `Unprobeable`.
  - `reads_an_advertisement`: a body of `001e# service=git-upload-pack\n0000` then two ref pkt-lines (one with `\0caps`) and a `^{}` line, then `0000`, gives three ids.
  - `#[ignore] network_public_and_private`: `probe("https://github.com/rust-lang/rust.git")` is `Public(ids)` with `ids` non-empty; `probe("https://github.com/beautifoo/no-such-repo-angerona.git")` is `Private`.
- [ ] **Step 2:** `cargo test public` fails.
- [ ] **Step 3:** Implement. In `test.yml`, run bats against the debug binary (`cargo build --locked` then `bats test/`, dropping `AG=…release…`), keep `cargo build --release --locked`, and add `cargo test --locked -- --ignored network` on both OSes (the release build's TLS is covered by building it with the same features).
- [ ] **Step 4:** `cargo test public` passes; `cargo test -- --ignored network` passes locally; global check.
- [ ] **Step 5:** Commit `feat: Tell whether a push URL is public without credentials`.

---

### Task 6: Compute what each commit adds

**Files:**
- Create: `src/added.rs`
- Modify: `src/main.rs` (`mod added;`)

**Interfaces:**
- Consumes: `text::{Item, Label, blob_item, name_item}`.
- Produces:
  - `pub fn new_lines<'a>(new: &'a [u8], olds: &[&[u8]]) -> Vec<(usize, &'a [u8])>`: 1-based line numbers of lines in `new` present in none of `olds` (set membership, not a diff). `olds` empty → every line.
  - `pub fn commit_items(repo: &gix::Repository, tree: gix::ObjectId, parents: &[gix::ObjectId]) -> Result<Vec<Item>, Box<dyn Error>>`: for each changed path against **every** parent tree (`repo.diff_tree_to_tree(parent, tree, gix::diff::Options::default().with_rewrites(Some(Default::default())))`), skipping a blob whose id appears in any parent tree (pure rename or move); old version = the same path in each parent, or the rename source; a text blob gives one `Item { label: Label::Plain(format!("{path}:{n}")), bytes: line }` per new line; a binary blob (NUL in first 8000 bytes) gives `blob_item(path, data)` whole; every added or renamed path gives `name_item("file", path)`. A root commit or a parent whose tree is missing (partial clone with `GIT_NO_LAZY_FETCH=1`): every line is new.

- [ ] **Step 1: Write the failing tests** for `new_lines`: `only_lines_in_no_parent` (`b"a\nb\nc\n"` vs `[b"a\nc\n"]` → `[(2, b"b")]`), `merge_keeps_only_lines_new_to_every_parent` (`b"a\nb\nx\n"` vs `[b"a\n", b"b\n"]` → `[(3, b"x")]`), `root_is_all_new`.
- [ ] **Step 2:** fails; **Step 3:** implement both functions (`commit_items` is exercised end to end in Task 7's bats tests); **Step 4:** passes, global check.
- [ ] **Step 5:** Commit `feat: Compute the lines a commit adds`.

---

### Task 7: The pre-push check

**Files:**
- Create: `src/check.rs`
- Modify: `src/push.rs`, `src/main.rs`, `test/angerona.bats`

**Interfaces:**
- Consumes: everything above.
- Produces:
  - In `push.rs`: `pub fn held(repo: &gix::Repository, remote: &str, url: &str, advert: Option<Vec<ObjectId>>) -> Vec<ObjectId>`: `Some(ids)` → those present locally; `None` → today's `list_refs` over the push transport, else the ids of `refs/remotes/<remote>/*`. Never prints "scanning the whole history" unless all three fail. `main` passes `Some(ids)` only for a non-empty advertisement: an empty one (a new repo, or the debug override) falls back to `list_refs`, which gives the same answer for an empty remote. `read` and `parents` become `pub` (Task 10 uses them).
  - `pub struct Outgoing { pub commits: Vec<(ObjectId, bool)>, pub tags: Vec<ObjectId>, pub blobs: Vec<ObjectId> }` (`bool` = unpublished; `blobs` = tags pointing at blobs or trees)
  - `pub fn outgoing(repo: &gix::Repository, refs: &[RefLine], held: &[ObjectId]) -> Result<Outgoing, Box<dyn Error>>`: today's walk (shallow-aware `parents`, `remote_commits(held)`), plus a commit is unpublished when not reachable from any `refs/remotes/*`; every tag object on a pushed ref is in `tags`, even when its commit is held.
  - In `check.rs`: `pub struct Finding { pub at: String, pub what: String, pub hint: &'static str }`; `pub fn commit_findings(repo: &gix::Repository, id: ObjectId, unpublished: bool, m: &Matcher, own: &[String]) -> Result<Vec<Finding>, Box<dyn Error>>` (`added::commit_items` through `m.find`; `identity::read` of the raw object, its idents' `raw` and its message through `m.find`; `identity::foreign`); `pub fn tag_findings(repo, id, m, own) -> Result<Vec<Finding>, Box<dyn Error>>` (same, for a tag object, and its blob or tree target whole via `text::blob_item` when it points at one); `pub fn push_findings(repo, out: &Outgoing, refs: &[RefLine], m: &Matcher, own: &[String]) -> Result<Vec<Finding>, Box<dyn Error>>` (the two above over `out`, plus each `RefLine.remote_ref` through `m.find_ref`); `pub fn render(f: &[Finding]) -> String` (`<at>  <what>` lines, then the fix hints once each). `own` is the addresses of `Details.emails`.
  - Hints: home path or login in a file → `edit the file, then amend or rebase`; own email or login@host in an identity → `git rebase -i <base> --exec 'git commit --amend --no-edit --reset-author'`; in a message → `git rebase -i, reword`; in a ref name → `push under another name: git push <remote> HEAD:refs/heads/<name>`.
  - `main.rs`: `check-push REMOTE URL` reads stdin once, parses ref lines (no refs → exit 0 without probing), sets `GIT_NO_LAZY_FETCH=1`, refuses grafts as today, probes `URL`; `Private` → exit 0; else findings; none → exit 0; some → print `render` and exit 1 (prompting comes in Task 8).

- [ ] **Step 1: Write the failing bats tests** (helpers as today; `export USER=jdoe ANGERONA_TEST_HOSTS=jdoes-laptop ANGERONA_TEST_VISIBILITY=public` in `setup`; the hook is installed in each test repo with `git config hook.angerona.event pre-push` and `git config hook.angerona.command "$AG check-push"`; pushes go to the local bare `origin.git`):
  - `a clean push passes`
  - `a home path in a new file blocks` (file content `/Users/jdoe/x`; output has `/Users/jdoe`)
  - `an old home path in an edited file passes` (pushed before with `ANGERONA_TEST_VISIBILITY=private`, then a new unrelated line added)
  - `a line added then removed in the same push still blocks`
  - `a merge conflict edit is checked` and `a merged-in side is not re-reported` (the side branch was already pushed)
  - `a pure rename passes` and `a renamed and edited file reports only the new line`
  - `a binary file with a home path blocks`, `a UTF-16 file with a home path blocks`
  - `a file path with the login blocks` (`x/jdoe.xcuserdatad/y`)
  - `my email as author blocks` (`git config user.email me@home.example` set globally after `GIT_CONFIG_GLOBAL` setup, commit made with `-c user.email=me@home.example` while global email is a noreply)
  - `my email in a Co-authored-by trailer blocks`
  - `an unknown committer email on a new commit blocks`, `an unknown author email on an applied patch passes` (author `bob@corp.example`, committer noreply), `an upstream contributor's email on a fork sync passes` (commit fetched from a second remote `upstream`, so it is published)
  - `a tag on a pushed commit is checked` (annotated tag with tagger `bob@corp.example` on `main`, pushed with `git push origin v1`)
  - `a branch named after my login blocks` (`jdoe/fix`), `the local branch name is not checked` (`git push origin jdoe/fix:refs/heads/fix` passes)
  - `a private remote is not checked` (`ANGERONA_TEST_VISIBILITY=private`, home path pushed, exit 0)
  - `a deletion-only push passes`
  - `a Latin-1 message still matches` (message bytes `caf\xe9 /Users/jdoe`)
  - `the hook works from a linked worktree`
  - `an unreadable object refuses the push` (corrupt a loose object; output has `refusing to push`)
- [ ] **Step 2:** `bats test/` fails on the new tests.
- [ ] **Step 3:** Implement `push.rs`, `check.rs` and the `check-push` arm.
- [ ] **Step 4:** `bats test/` passes; global check.
- [ ] **Step 5:** Commit `feat: Check what a push sends to a public repo`.

---

### Task 8: Ask before pushing, and about what can't be checked

**Files:**
- Create: `src/tty.rs`
- Modify: `src/main.rs`, `test/angerona.bats`

**Interfaces:**
- Produces: `pub fn ask(question: &str, default_yes: bool) -> Option<bool>`: opens `/dev/tty` read-write; `None` when it can't, or `TERM` is unset or `dumb`; writes the question, reads one line; empty → default; `y`/`yes` → true; `n`/`no` → false (case-insensitive); anything else asks again. Never reads stdin.
- `check-push` flow after this task:
  1. `Private` → exit 0.
  2. `Unprobeable`: `ask("Treat <url> as public? [Y/n]", true)` on every push, saving nothing; `Some(false)` → exit 0; `Some(true)` or `None` → check.
  3. `Passing` → check as public, never ask.
  4. On findings: print them; `ask("Push anyway? [y/N]", false)`; `Some(true)` → exit 0; `Some(false)` → exit 1; `None` → print `no terminal to ask; to push once past this: git push --no-verify (it also skips every other pre-push hook)` and exit 1. Before asking, print `other hooks may still refuse this push`.

- [ ] **Step 1: Write the failing tests:** a unit test `answers_are_read` for the parsing helper (`fn parse(answer: &str, default_yes: bool) -> Option<bool>`, `None` meaning ask again) and bats tests run under `script -q /dev/null` (macOS) or `script -qc` (Linux) to give the hook a terminal, wrapped in a helper `with_tty INPUT CMD…`:
  - `answering y pushes past a finding`, `the default is no`
  - `without a terminal a finding blocks and names --no-verify` (plain `run`, output has `--no-verify`)
  - `an unprobeable URL asks on every push and saves nothing` (answering n passes, the next push asks again, and no `angerona.` key is in the repo config)
  - `a passing failure checks without asking`
- [ ] **Step 2:** fails; **Step 3:** implement; **Step 4:** passes, global check.
- [ ] **Step 5:** Commit `feat: Ask before pushing past a finding`.

---

### Task 9: angerona setup

**Files:**
- Create: `src/setup.rs`
- Modify: `src/main.rs`, `test/angerona.bats`

**Interfaces:**
- Produces: `pub const HOOK_COMMAND: &str = "f() { if command -v angerona >/dev/null 2>&1; then angerona check-push \"$@\"; else echo 'angerona not installed, push not checked' >&2; fi; }; f";` (git appends ` "$@"` to a config hook's command, so the trailing `f` receives the remote and URL). `pub fn run() -> i32`: `git config set --global hook.angerona.event pre-push` and `git config set --global hook.angerona.command "<HOOK_COMMAND>"` through `std::process::Command` (git follows a symlinked config file and a symlinked directory); prints what it set. Then `gh api user/emails --jq '.[].email'`: success → lowercase, drop noreply (`details::is_noreply`), write to `details::emails_path()` (create the directory 700, the file 600, replace atomically via a temp file in the same directory and rename), print how many were saved; `gh` not on PATH or not logged in → print `gh not available: only the emails in your git config are known` and leave any existing file; output names a missing scope (gh prints `needs the "user" scope` for HTTP 404, since GitHub accepts `user` or `user:email`) → print `run: gh auth refresh -h github.com -s user:email, then angerona setup again` (never suggest the broader `user` scope). Exit 0 when the hook was written (the gh part never fails setup), 1 with git's error otherwise.

- [ ] **Step 1: Write the failing bats tests:**
  - `setup writes the hook into the global config` (`git config --global hook.angerona.command` equals the constant; `hook.angerona.event` is `pre-push`; no `/Users/` or `$HOME` text in the global file)
  - `setup writes through a symlinked config` (`GIT_CONFIG_GLOBAL` points at a symlink; after setup it is still a symlink and the target holds the lines)
  - `the hook line runs angerona from PATH` (with `PATH` containing a dir with `angerona` → `$AG`, a push with a home path is refused)
  - `without angerona the push passes with a warning` (PATH without it; push exit 0, output has `angerona not installed, push not checked`)
  - `setup saves my GitHub emails privately` (a fake `gh` script first on PATH prints `Me@Personal.Example` and `1+me@users.noreply.github.com`; the emails file holds only `me@personal.example`, mode `600`; a later push adding that address to a file is refused)
  - `setup asks for the user:email scope` (fake `gh` exits 1 printing `This API operation needs the "user" scope`; output has `gh auth refresh -h github.com -s user:email`; exit 0)
  - `setup without gh still writes the hook` (no `gh` on PATH; output has `gh not available`; exit 0)
- [ ] **Step 2:** fails; **Step 3:** implement; **Step 4:** passes, global check.
- [ ] **Step 5:** Commit `feat: Write the hook with angerona setup`.

---

### Task 10: angerona scan

**Files:**
- Create: `src/scan.rs`
- Modify: `src/main.rs`, `test/angerona.bats`

**Interfaces:**
- Consumes: `push::{read, parents}`, `check::{commit_findings, tag_findings, Finding, render}`, `text::blob_item`, `matcher::Matcher`.
- Produces: `pub fn run(repo: &gix::Repository, m: &Matcher, own: &[String]) -> i32`: every commit reachable from every ref (`repo.references()?.all()?`) through `commit_findings(.., unpublished = true, ..)`; every annotated tag; every ref name through `find_ref`; every file in `HEAD`'s tree, whole (`blob_item`), as `current <path>:<n>`. Shallow (`repo.shallow_commits()` non-empty) or partial (`remote.*.promisor` or `extensions.partialclone` set) → print `angerona: this clone is shallow or partial; run git fetch --unshallow (or a full clone) first, or the report misses old commits` before the report. Progress `angerona: scanned N commits…` on stderr every 2 s while running. Exit 0 when clean, 1 with findings. History hints add: `fix history with git filter-repo (keep its mailmap file outside the repo)`.

- [ ] **Step 1: Write the failing bats tests:** `scan of a clean repo passes`, `scan reports an old home path deleted since` (commit adds, next commit removes), `scan reports a current file`, `scan reports any committer email` (`bob@corp.example` committer), `scan warns on a shallow clone`, `scan never changes the repo` (`git rev-parse --all` and `git status --porcelain` same before and after).
- [ ] **Step 2:** fails; **Step 3:** implement; **Step 4:** passes, global check.
- [ ] **Step 5:** Commit `feat: Scan a repo's whole history before it goes public`.

---

### Task 11: angerona doctor

**Files:**
- Modify: `src/doctor.rs`, `src/main.rs`, `test/angerona.bats`

**Interfaces:**
- Consumes: `details::{machine, discover, is_noreply}`, `setup::HOOK_COMMAND`, the kept `check_gits`.
- Produces: `pub fn run() -> i32`. Checks, each a problem line with its fix command, in this order:
  1. git ≥ 2.54 (kept).
  2. `git config --global user.useConfigOnly` is `true`; fix `git config set --global user.useConfigOnly true`.
  3. The global `user.email` is set and noreply; fix names the host's noreply form (`<id>+<user>@users.noreply.github.com`).
  4. At least one `includeIf` section in the global config; else a note on `includeIf "hasconfig:remote.*.url:…"` per host.
  5. `hook.angerona.command` equals `HOOK_COMMAND` and `hook.angerona.event` is `pre-push`; fix `angerona setup`.
  6. `env PATH=/usr/bin:/bin:/usr/sbin:/sbin sh -c 'command -v angerona'` finds it (the PATH apps started from the Dock give hooks); else a note: `apps started from the Dock won't find angerona, so their pushes print 'not checked'; link it into /usr/local/bin`.
  7. `user.signingkey` does not look like an email (contains `@`); fix: set it to the key fingerprint.
  8. The emails file exists and holds the same non-noreply addresses `gh api user/emails --jq '.[].email'` returns now; missing or different → problem with fix `angerona setup`; gh unavailable or lacking the scope → a note, not a problem.
  9. Every line of `Details.weak` as a note (weak login or generic machine name; suggest `sudo scutil --set HostName <name>` on macOS).
  Then lists the emails found with where, a note `turn on your forge's email privacy setting (GitHub: Keep my email addresses private), since commits made on the website skip the hook`, and the last line `repos already public are not rechecked; run angerona scan in each`. Exit 0 with no problems, 1 otherwise (notes alone don't fail).

- [ ] **Step 1: Write the failing bats tests:** `doctor passes a good setup`, `doctor flags a stale emails file` (fake `gh` returns an address the file lacks), `doctor flags a missing useConfigOnly`, `doctor flags a real global email`, `doctor flags a signing key set as an email`, `doctor flags a missing hook`, `doctor notes a weak login` (`USER=dev`), `doctor with no identity says how to set one` (no `user.email`, `HOME` set to an empty dir), `doctor ends with the scan reminder`.
- [ ] **Step 2:** fails; **Step 3:** implement; **Step 4:** passes, global check.
- [ ] **Step 5:** Commit `feat: Check the git settings that keep you out`.

---

### Task 12: Linux release builds

**Files:**
- Modify: `dist-workspace.toml` (`targets = ["aarch64-apple-darwin", "x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl"]`), `.github/workflows/release.yml` (regenerated)

- [ ] **Step 1:** Edit `targets`. Run `dist generate` (cargo-dist 0.34.0). If `dist` is not installed, stop and ask the owner before installing anything.
- [ ] **Step 2:** `dist plan` lists the three targets; `git diff .github/workflows/release.yml` shows only target-matrix changes and keeps the pinned action commits from `[dist.github-action-commits]`.
- [ ] **Step 3:** Global check (no Rust changes, so it stays green).
- [ ] **Step 4:** Commit `ci: Build release binaries for Linux`.

---

### Task 13: Document the new angerona

**Files:**
- Modify: `README.md`, `GLOSSARY.md` (only if a task found a term missing), `HANDOFF.md` is not touched

- [ ] **Step 1:** Write `README.md` for the new scope: what it keeps out (the spec's "What counts as you"), install (Homebrew tap `beautifoo/tap/angerona`, or `cargo install --git`), `angerona setup`, `angerona doctor`, what the hook does and asks, `angerona scan` before making a repo public, what is not checked (the spec's accepted gaps: internal repos, LFS and compressed files, forge web commits, others' emails in authors and content), and how to skip once (`git push --no-verify`, which also skips other pre-push hooks). Requirements: git ≥ 2.54, macOS or Linux.
- [ ] **Step 2:** Every command shown in the README runs as written against the debug build in a scratch repo; record nothing, just check.
- [ ] **Step 3:** Global check; commit `docs: Describe keeping you out of what you publish`.
- [ ] **Step 4:** Report to the owner the GitHub description and topics to set (`Keeps your emails, home path and machine name out of what you push to public repos`; topics `git`, `git-hooks`, `privacy`, `pre-push`, `rust`) and wait for a yes before running `gh repo edit`.
