# angerona: keep you out of what you publish

Design, 2026-11-18. Replaces the old scope (a private list of work terms kept out of commits, pushes and MCP calls), which gitleaks now covers for the owner.

## Intent

angerona keeps the person behind the code out of what they publish with git. Not secrets (gitleaks and friends do those) but **personal details**: your emails, your home path, your login name, your machine's name.

The owner already commits with his GitHub noreply address so his personal email stays private. angerona makes sure nothing slips past that habit: a commit made before the alias was set, a cherry-pick or `git am` from a work clone that keeps the work email, a `Co-authored-by` trailer, an agent writing `/Users/<login>/...` into `.mcp.json`, a terminal prompt `login@host` pasted into a README.

It must be useful to other developers, not only the owner. The first version protects **one developer, on their own machine**.

## Goals

- Zero setup for what counts as you: everything is found on the machine.
- Check emails only in what goes to a **public repo**, or to one angerona can't show is private (no terminal to ask on, or the host didn't answer). Pushes to private repos (work, personal) are checked only for the home path, login and machine name, which make sense on this machine alone.
- One hook, at push time, the last local point before publishing. No commit hook: personal details rarely change, and a push check catches both carried-over commits and file contents.
- A one-off `scan` for an existing repo before making it public.
- A `doctor` that checks the git settings that prevent most leaks in the first place.

## Non-goals (first version)

- CI checks for other projects, and a GitHub Action. (An Action comes later.)
- Windows paths and builds.
- A hand-written list of extra details (old usernames, other emails). The old angerona was that list. Emails come from git config.
- Release files and tarballs, paths inside shipped binaries.
- Agent and MCP checks.
- Rewriting history. `scan` reports; `git filter-repo` fixes.
- Secrets of any kind.

## Anti-goals

- **Prompt fatigue.** If the hook asks on ordinary pushes (fork syncs, patches from others), people learn to answer y without reading, and the tool is worse than nothing. Every rule below that narrows what is flagged exists for this.
- **Failing open by accident.** An error while walking or reading must never read as "nothing to check". The one deliberate pass-through is a machine without angerona installed, and it says so on every push.
- **Leaking through angerona itself.** Lines it writes to the global config (often in a public dotfiles repo) hold no home path; its public check sends no credentials anywhere.
- **Editing the user's config beyond what was asked.** Only `setup` writes the global config; the hook writes no config.

## Language

See `GLOSSARY.md`: **personal detail**, **outgoing commit**, **unpublished commit**, **public repo**, **noreply address**.

## What counts as you

- **Emails:** every `user.email` in every git config file: system, global, and every file reached through `include` and `includeIf`, *whatever the condition* (so the work email set only under `~/Developer/work/` is known from a personal repo too), following nested includes, relative paths resolved from the including file. Plus the `EMAIL`, `GIT_AUTHOR_EMAIL` and `GIT_COMMITTER_EMAIL` environment variables. The emails come from git config and these variables only: no GitHub lookup and no emails file by default. A real personal email that no git config on this machine holds is caught only as the committer of an unpublished commit or the tagger of a pushed tag. Reading your GitHub account's emails (`gh api user/emails`, kept in `~/.config/angerona/emails`) is an experimental build flag, `--features github-emails`, off by default and being reworked. Noreply addresses are never personal details. **The repo's own email**, the email git uses in this repo, when it differs from the one git uses outside any repo, is never reported in that repo, by any rule, in the hook or in `scan`. The first is `git config --get user.email` in the repo, with every `includeIf` resolved by git; the second is the same command run from `/` with `GIT_DIR` and `GIT_WORK_TREE` removed, so the system, global and XDG files and every plain `include` count while no `includeIf` on a gitdir or branch matches (one on `hasconfig:remote.*.url` still does when the global config itself names a matching remote). They are compared case-insensitively. A repo that sets none gets the email git uses outside any repo, which is never skipped, so a real global address, even one in an included file, is still caught everywhere; every other email of yours still is, so a work email in a personal repo whose own email is a noreply address is still caught. When no email is set outside any repo (git exits 1 and prints nothing), any email the repo sets differs from it. If either git call fails in any other way, nothing is left out.
- **Home path:** `realpath($HOME)` and the `/Users/<login>` or `/home/<login>` forms, matched case-insensitively and in escaped forms: `\/` (JSON), `\` and `\\`, `%2F`, `file:///`, `~<login>/`. No full decoding step: one pattern.
- **Login name:** only right after a home root (`Users`, `home`, `~`), in the `<login>.xcuserdatad` form, in `<login>@<hostname>` text (with or without a domain, the macOS shell prompt form), and **as a branch-name prefix** (`<login>/feature`): the owner decided this is a leak. Never as a bare word or a lone path part (`/dev/null` must not match a `dev` login). Logins shorter than 3 characters or generic (root, admin, user, dev, test, pi, ubuntu, runner, and the like) get the same floor as machine names: not matched by the login rules (the home path itself still is), and `doctor` says so.
- **Machine name:** every form macOS has (`scutil` HostName and LocalHostName, plus `hostname`), case-insensitive, `.local` and domain stripped. ComputerName is skipped (spaces, full name). Matched only as `<login>@<host>` or as the full domain label after `@` (`x@host.local`), never as any label of any domain. Names shorter than 4 characters, `localhost`, and generic names (Mac, ubuntu, raspberrypi, and the like) are not matched; `doctor` says so and suggests setting a unique name.
- **Not your full name:** it is on every commit anyway.

### Noreply addresses

An address is noreply when its local part is `noreply` or `no-reply`, or its domain starts with `noreply.` or `users.noreply.` (GitHub `<id>+<user>@users.noreply.github.com`, GitLab `users.noreply.<host>`, Forgejo `noreply.<domain>`, bot addresses like `noreply@anthropic.com`). This is a heuristic, on purpose: no fixed domain list can cover self-hosted forges. The noreply test runs on the whole address first, and no login or hostname matching runs inside a noreply address.

## The pieces

### `angerona setup`

Writes the pre-push hook into the global git config (git ≥ 2.54 config-based hooks: `hook.angerona.event = pre-push`, `hook.angerona.command = …`). The command line holds no home path: it runs bare `angerona`. When `angerona` is not on the PATH git gives the hook (from a terminal or an app), the line prints `angerona not found, push refused: …` and exits 1, so no push goes out unchecked (decided 2026-11-24; it used to let the push through with a note). Putting angerona on that PATH is the machine's setup, not angerona's: a machine that gets the global config through dotfiles needs angerona or the hook lines removed, and on macOS, apps started from the Dock get launchd's PATH, which `sudo launchctl config user path …` and a reboot change.

Setup never sets `parallel = true`. If the global config file is a symlink (dotfiles), setup writes through it rather than replacing it. If the first git on the PATH is older than 2.54 (or there is none), git would never run the hook, so setup writes nothing, says so, points at `angerona doctor` for the fix, and exits 1.

Only with `--features github-emails`: setup also reads your GitHub account's emails with `gh api user/emails` and writes the non-noreply ones to the emails file (mode 600). When `gh` is missing or not logged in, it says so and carries on with the config emails only. When the token can't read the emails, it prints `run: gh auth refresh -h github.com -s user:email, then angerona setup again`. `user:email` only lets gh read your email addresses; GitHub also accepts the broader `user` scope (it can change your profile), which angerona never asks for. angerona never changes your gh login itself. The hook reads only the file: no `gh` or network call per push for this.

### Pre-push hook

**1. Is the target public?** Judge the URL git passes as the hook's second argument (it already has `pushurl` and `pushInsteadOf` applied; git runs the hook once per push URL). Take its https form: SSH forms are mapped by resolving the host with `ssh -G <host>` (stdin closed) and reading `hostname`, dropping the SSH port, mapping `ssh.github.com` to `github.com`. Then make **one plain HTTP GET** of `<https-url>/info/refs?service=git-upload-pack`, with no credentials of any kind (user and password stripped from the URL; no netrc, no git credential helpers, no `insteadOf`; not through `git ls-remote`, which was shown to send `GIT_ASKPASS`, netrc and URL-scoped header credentials, and to follow `insteadOf` to SSH), following redirects, `--connect-timeout 3 --max-time 5` or equivalent.

- **Public:** final 200 with content type `application/x-git-upload-pack-advertisement`.
- **Private:** final 401, 403 or 404, or a local path or `file://` URL (your own disk). The push is checked for the home path, login and machine name only: no emails, yours or others'.
- **Can never be probed** (a steady answer that isn't git's): an SSH host with no https mapping, connection refused, a sign-in or other HTML page, a 200 with another content type, a TLS certificate error (self-signed internal host).
- **Passing failure:** timeout, DNS failure, 5xx, 429.

For a URL that can never be probed: ask on `/dev/tty` on every push, `Treat <url> as public? [Y/n]`, and save nothing. With no terminal: check as public. After a passing failure, **never ask**: check the push as public. A URL answered as private is checked as private (above).

The response body of the public check is the remote's ref list, so it also gives what the remote already has (step 2) with no extra request. When a public push has no such list (a passing failure, or a URL answered or assumed public), list the remote's refs over the push transport itself, with the user's normal credentials, as `git push` does (today's `push::held`); if that fails too, use that remote's remote-tracking refs. A private push never opens the transport, which could ask for a password or hang: it uses that remote's remote-tracking refs, and with none, scans each pushed ref's whole history. The no-credentials rule is for deciding public or private; this listing goes to the place the push goes anyway.

**2. What goes out.** The remote's refs from step 1, kept only where the object exists locally, are what the remote has. Outgoing commits are the commits reachable from the pushed local shas and not from those. Remote shas from the hook's stdin are only a hint; a remote sha missing locally is ignored, never an error. Of those, the **unpublished commits** are the ones no remote-tracking ref of any remote reaches. Annotated tags in the push are read directly as objects (also when they point at a commit the remote already has, or at a blob or tree). Partial clones: set `GIT_NO_LAZY_FETCH=1` or the gix equivalent, and treat a missing parent blob as "every line is new".

**3. What is checked, per outgoing commit:**

- **Added lines:** lines in each changed file that are not in that file's version in *any* parent (set difference, not a diff algorithm; covers merge-conflict edits without re-reporting the merged-in side). A blob that already exists in a parent's tree adds nothing (a pure rename or move). A renamed and edited file is compared with its rename source (gix rename tracking). A root commit or a missing parent: every line is new.
- **File paths** of every added or renamed file.
- **Binary blobs** (as git decides binary): read as their readable runs of 6 or more characters, in ASCII and in UTF-16 at both byte alignments; a blob that starts with a UTF-16 byte-order mark is decoded whole.
- **Identities and trailers:** author, committer, tagger, and every `Name <email>` trailer (`Co-authored-by`, `Signed-off-by`, `Reported-by`, `Reviewed-by`, `Helped-by`, …), read from the raw object, never through mailmap-aware formats.
- **Commit and tag messages**, signatures stripped.
- **The remote ref name**, only when the push creates the ref (the hook's remote sha is all zeros): a name the remote already has is there already, and the local name never reaches the remote.

**Email rule:** your emails are flagged anywhere in any outgoing commit: identities, trailers, messages, lines, paths, binaries. Any other non-noreply email is flagged **only as the committer of an unpublished commit or the tagger of a tag in the push**, the fields that say who ran git. So fork syncs and other people's already-public commits stay quiet; a patch you apply (`git am`, a cherry-pick of a contributor's branch) has you as committer and the contributor as author, and stays quiet; a commit made on another machine under a personal email set nowhere on this one is still caught. Other people's emails in authors, trailers and content are never flagged. The repo's own email is skipped only when it is shaped like an address (text, `@`, text): a `user.email` of `jdoe` would otherwise hide a `jdoe/` branch or a login finding. Home path, login and machine-name rules apply to all outgoing content, in private repos too. The email rule applies only to a public repo.

**4. On a finding:** list each finding (commit, where, what, a fix hint), then with a terminal (`/dev/tty` opens) ask `Push anyway? [y/N]`, default No. Never read the answer from stdin (it holds the ref lines). Without a terminal: block, leading with the fix and leaving the bypass to a person: `fix the lines above and push again; only a person should push past this, with git push --no-verify (it also skips every other pre-push hook)`. The prompt mentions that other hooks may still refuse the push. Any internal error blocks.

### `angerona scan`

Run by hand in an existing repo, typically before making it public. Checks every commit in the history (all refs) with the same per-commit rules, treating every commit as unpublished, plus every file in the current tree, binaries included. Another person's non-noreply committer or tagger email is reported only when it looks machine-guessed (its domain ends in `.local`, `.lan`, `.localdomain` or `.(none)`, or has no dot: git's `login@host` fallback) and its local part is your login, case-insensitively: your own old machine. A colleague's guessed address never shows. An empty address is never reported. `angerona scan --all-emails` reports every one. The push hook is unchanged and still flags them all. Output is grouped: first every home path, login and machine-name finding, one line per place; then one line per email address (yours first, then by count, highest first) with the fields it appears in, how many commits or tags, and the first and last in scan's walk order; then each fix hint once. Reports only, with fix hints: `git filter-repo` for history (with the mailmap kept outside the repo, since a committed `.mailmap` would publish the old address), edits for current files. A finding in a commit any remote-tracking ref already reaches gets `already pushed: fix it in a new commit; rewriting shared history needs everyone who has it` instead of a rewrite, a current file gets `edit the file and commit`, and the filter-repo line shows only when some finding still needs a rewrite. Warns loudly on a shallow or partial clone ("fetch --unshallow first"). Prints progress after about 2 s.

### `angerona doctor`

Checks and prints what to fix:

- `user.useConfigOnly = true` (stops git guessing `login@host`).
- An identity per host via `includeIf` (gitdir or `hasconfig:remote.*.url`).
- The global `user.email` is a noreply address; lists every email angerona found and where.
- In a repo with its own email, marks it in that list as never reported there (see the repo's own email under Emails); one set only in the repo's own config is added to the list with that mark, unless it is a noreply address.
- With `--features github-emails` only: the emails file exists and matches what `gh api user/emails` returns now; when it differs or is missing, says to run `angerona setup` again (skipped with a note when `gh` can't answer).
- The hook is installed, git is ≥ 2.54.
- angerona is on the PATH doctor runs with (the PATH the hook gets for pushes from that shell); missing is a problem.
- `user.signingkey` (or gpg's default key) is not an email address (a GPG key given as an email embeds that email in every signature); suggest the fingerprint.
- The machine name is unique enough to match (see above).
- A reminder to turn on the forge's email privacy setting (GitHub "Keep my email addresses private"), since commits made on the website never pass through the hook.
- One closing line: repos already public are not rechecked; run `angerona scan` in each.

## Constraints

- Rust, gix 0.90, on top of the current `main`. No new dependency for the HTTP check: the curl crate is already in the tree through gix's transport.
- macOS and Linux. Release builds for Linux (static) are added to `dist-workspace.toml`; today it builds `aarch64-apple-darwin` only.
- git ≥ 2.54 for config-based hooks (Apple git 2.54 also runs them).
- Many users keep their global git config in a public dotfiles repo.
- angerona's own CI stays: `test.yml`, the cargo-dist release, attestations, the Homebrew tap. `HOMEBREW_TAP_TOKEN` must be renewed before 2027-11-19.

## Approaches

- **A. Build on `main`, read git in-process with gix. Picked.** Keep the 43 published commits; delete the old guard's modules; reuse the push walk (`push.rs`: ref listing, outgoing commits, headers), the binary/UTF-16 text-run reader (`scan.rs`) and parts of `doctor.rs`. Added lines by set difference against each parent, which needs no diff algorithm and covers merges (gix-diff 0.70 has no combined diff). Fast (the current walk takes about 0.03 s on an 8 MB repo) and no parsing of git's text output.
- **B. Build on `main`, run `git log -p --cc --no-mailmap …`.** Exact git semantics for merges and binaries, less code of our own, but parses git's text output and was slow on big histories (about 12 s and 375 MB of output for a 14k-commit repo). Not picked.
- **C. Fresh history, force-push over `main`.** Clean story, but rewrites 43 public commits for tidiness only; the old guard is already public and the history explains the pivot. Not picked.

## Decided by the owner

- The new scope, and that the first version protects one developer on their own machine.
- Three pieces: `doctor`, pre-push hook, `scan`; plus `setup` to write the hook.
- No commit hook. Emails checked only for public repos; private is fine for them, and `scan` covers a repo going public later.
- "Can't tell" → ask on every push, only for hosts that can never be probed, default Y: the stricter check, so answering by habit stays safe; no terminal → check as public.
- Your emails flagged anywhere in any outgoing commit; others' non-noreply emails only in commit identities, and only as committer of an unpublished commit or tagger (so applied patches stay quiet). Same in `scan`, except that `scan` reports another person's email only when it looks machine-guessed from your login (see `angerona scan`).
- `scan` groups its output and skips other people's chosen emails (2026-11-19): on a team repo, thousands of committer and tagger emails drowned the few real findings. The hook is unchanged.
- The hook also checks pushes to private repos, for the home path, login and machine name only (2026-11-19): a `/Users/<you>` path only makes sense on your machine, so it is wrong in any shared repo, private or not. Emails are still checked only for public repos.
- The repo's own email is never reported in that repo (2026-11-19): a work repo rarely goes public, and if it does, its work email is the right identity for it. Only when the repo's email differs from the one git uses outside any repo: a repo with no email of its own gets that one, and a real global address must still be caught. Comparing with the top-level global value alone (2026-11-19) missed an email set in a plain include or the system config, and so skipped it in every repo.
- Other people's machine-guessed emails are skipped by `scan` unless their local part is your login (2026-11-19): a colleague's guessed address (like `pat@pats-laptop.local`) names their machine, not you.
- No way to allow a home path, login or machine name on purpose in a repo: only added lines are checked, so answering y is enough. The one email never reported in a repo is its own (above).
- Superseded by the next line, kept for the `github-emails` follow-up: your real emails come from your GitHub account (`gh api user/emails`), read once by `setup` into a private file; the owner adds the read-only `user:email` scope to gh himself when setup asks (GitHub's `X-Accepted-OAuth-Scopes` for `/user/emails` is `user, user:email`, checked 2026-11-18). (Reopened 2026-11-18 after the email rule narrowed: without it a personal email set in no git config would pass as an author, in trailers and in files.)
- By default your emails come from git config only (every `user.email` and the environment variables); the GitHub-account lookup is kept behind `--features github-emails`, off by default (2026-11-19). A saved file goes stale, an API call per push is not wanted, gh's cache can't be located without the token, and a privacy tool must not keep emails as plain text; the follow-up redoes the storage.
  Consequence: an email set in no git config on this machine is caught only as the committer of an unpublished commit or the tagger of a pushed tag.
- The login as a branch prefix is a personal detail.
- Added lines only in the hook; whole history and files in `scan`.
- On a finding: `Push anyway? [y/N]`; without a terminal, block, say to fix the lines and push again, and that only a person should push past it with `--no-verify` (2026-11-19).
- Missing binary: refuse the push (2026-11-24; it used to warn and push).
- Generic machine name: `doctor` tells you.
- `doctor` does not search the disk for public repos; one closing line instead.
- `scan` reports only.
- macOS and Linux; no CI checks, Windows, extras list, release files or agent checks in this version.
- Keep angerona's own CI, release pipeline and tap.
- Approach A.
- The old local branches `bash-main` and `feat/rust-rewrite` are deleted (done 2026-11-18).

## Calls made in design (owner can override)

- The public check is a plain HTTP GET, not `git ls-remote` (credential leaks shown in testing).
- Matching forms for home path, login and machine name above, and the noreply heuristic.
- Fail closed on every internal error and on a missing binary.
- Identities read from raw objects (mailmap would hide real emails).
- `scan` applies the unpublished-commit rule for other people's emails to every commit, since going public publishes them all; a commit a remote-tracking ref reaches only gets another fix hint.
- The public-check outcome table (public, private, can never be probed, passing failure) above.
- With no ref list from the public check, list refs over the push transport (public pushes only), then fall back to remote-tracking refs.
- Pure renames add nothing; renamed and edited files compare with their source.
- Short and generic logins get the machine-name floor.
- No migration from the old hooks: nothing was released, and no machine runs them.
- Accepted and documented gaps: "internal" enterprise repos count as private, so emails pushed there are not checked; LFS content, compressed files (zip, gzip, images) and commits made on the forge's website are not checked; a commit already on a private remote and pushed to a public one is checked for your emails and paths but not for others' emails.
- ADR 0001 (one hook name per event) no longer applies: there is one hook. Mark it superseded.

## Left to the builder

- Module layout, function names, output wording and colours.
- How the hook command line guards a missing binary (an `sh` one-liner or similar).
- Test layout (unit tests plus bats, as today) and fixture design.
- The order of commits on `main`, as long as each one builds and passes tests.
- README, `Cargo.toml` description, and the GitHub description and topics for the new scope (changing GitHub needs the owner's yes).
