# angerona

Keeps you out of what you publish with git. Before a push, it checks that
nothing in it names you: your emails (when the repo is public), your home
path, your login name or your machine's name.

Secrets are for gitleaks and friends. angerona covers the details that point
back at you, such as a work email on a cherry-picked commit, a `Co-authored-by`
trailer, `/Users/<login>/...` in an agent's `.mcp.json`, or a `login@host`
prompt pasted into a README.

Angerona was the Roman goddess shown with her mouth sealed, who kept the secret
name of Rome.

## Requirements

git 2.54 or later, on macOS or Linux. An older git skips config-based hooks
without a word. `angerona doctor` names each git on your PATH that would.

## Install

With Homebrew, on macOS or Linux:

```sh
brew install beautifoo/tap/angerona
```

Or from source, with Rust 1.88 or later. On Linux this also needs a C
compiler, make and perl, to build curl and OpenSSL into the binary.

```sh
cargo install --locked --git https://github.com/beautifoo/angerona --tag v0.1.0
```

## Set up

```sh
angerona setup
```

This writes one pre-push hook into your global git config
(`hook.angerona.event` and `hook.angerona.command`). The hook line holds no
home path, so it is safe in a public dotfiles repo. It runs `angerona` from
the PATH git gives the hook. Where angerona isn't on that PATH, as on a
machine without it, the hook refuses the push, so nothing goes out unchecked:
install angerona there, or remove the two `hook.angerona` lines.

On macOS, apps started from the Dock, such as Xcode, don't get your shell's
PATH. Give them one that holds angerona's folder (Homebrew's is shown; for a
cargo install, add the full path of `~/.cargo/bin`, since launchd doesn't
expand `~`), then restart:

```sh
sudo launchctl config user path /opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin
```

Then check your git settings:

```sh
angerona doctor
```

`doctor` prints problems, each with its fix, and notes. It checks
`user.useConfigOnly`, a noreply global `user.email`, an identity per host with
`includeIf`, the hook, that angerona can be found on your PATH, a signing key
that is an email, a machine name too short or generic to match, and your git
version.
It exits 1 when it finds a problem; notes alone don't fail.
It lists the emails it keeps out of pushes and where it found each. In a repo
with its own email, it marks that email, or adds it when only the repo sets it:
that email is never reported in that repo.

## What counts as you

All of it is found on the machine, with nothing to list by hand.

- **Emails:** every `user.email` in every git config file, including each
  `include` and `includeIf` whatever its condition, plus `EMAIL`,
  `GIT_AUTHOR_EMAIL` and `GIT_COMMITTER_EMAIL`. A noreply address (GitHub's
  `<id>+<user>@users.noreply.github.com` and the like) is safe to publish and
  never counts. An email set in no git config on this machine is caught only as
  the committer of an unpublished commit or the tagger of a pushed tag.
  The repo's own email, the email git uses in this repo, when it differs from
  the one git uses outside any repo, is never reported in that repo: it is
  the identity the repo is meant to carry. A repo that sets none gets the one
  git uses outside any repo, which is still reported. Your other emails still
  are.
- **Home path:** your home directory, `/Users/<login>` and `/home/<login>`,
  also in escaped forms such as JSON's `\/`, `%2F` or `file:///`.
- **Login name:** after a home path, in `<login>@<host>`, as
  `<login>.xcuserdatad`, and as a branch prefix (`<login>/feature`).
- **Machine name:** in `<login>@<host>` and as the domain after an `@`.

Your full name doesn't count: it is on every commit anyway. A login or machine
name that is short or generic, such as `dev` or `Mac`, is skipped, and
`doctor` says so.

## The hook

On `git push`, the hook first asks whether the repo you push to is public. It
makes one plain request, with no credentials, to the repo's address. A repo
anyone can read is public. A private repo, a local path or a `file://` URL is
private.

It checks only what the push sends that the remote doesn't have yet: added
lines, file names, binary files, authors, committers, taggers, trailers such
as `Co-authored-by`, commit and tag messages, and the remote branch name.

- Home path, login and machine name are flagged everywhere, in private repos
  too: they only make sense on your machine.
- For a public repo, your emails are flagged anywhere in that.
- For a public repo, another person's email is flagged only as the committer
  of an unpublished commit, or the tagger of a tag. A fork sync or a patch you
  apply stays quiet.

Each finding says where it is and how to fix it. Then, with a terminal, it
asks:

```
Push anyway? [y/N]
```

The default is No. Without a terminal the push is blocked, with:

```
angerona: fix the lines above and push again; only a person should push past this, with git push --no-verify (it also skips every other pre-push hook)
```

If it can't tell whether a host is public (an SSH host with no web address, a
self-signed certificate), it asks on every push:

```
Treat <url> as public? [Y/n]
```

and saves nothing. Answering n treats the repo as private. With no terminal to
ask on, it checks the push as public. When the host can't be found, doesn't
answer in time, or answers with a server error or 429, it never asks: it checks
the push as public. Any error while reading the push blocks it.

To push once past the hook:

```sh
git push --no-verify
```

That also skips every other pre-push hook.

## Before making a repo public

```sh
angerona scan
```

Run it in the repo. It checks every commit on every ref and every file in the
current tree, and checks every commit as unpublished, since going public
publishes them all. Your details are listed first, one line per place, then one
line per email address with how often it appears. Other emails are reported only when they look machine-guessed from
your login, as an old machine of yours would make them (`<login>@host`,
`<login>@host.local`); `angerona scan --all-emails` lists them all. It changes nothing:
it reports, and tells you how to fix it. History is fixed with `git filter-repo`, keeping its mailmap file
outside the repo so the old address isn't published. A commit a remote already
has gets a new commit instead, since rewriting shared history needs everyone
who has it, and a current file is edited and committed. A shallow or partial
clone gets a warning to fetch the full history first.

Exit 0 means nothing found, 1 means findings or an error.

## What it won't catch

- "Internal" repos of an enterprise host count as private, so emails pushed
  there aren't checked.
- LFS content and compressed files (zip, gzip, images) aren't read.
- Commits made on the forge's website skip the hook. Turn on the forge's email
  privacy setting (GitHub: Keep my email addresses private).
- Other people's emails in authors, trailers and file content aren't flagged.
- A commit already on a private remote and pushed to a public one is checked
  for your emails and paths, not for others' emails.
- Repos already public aren't rechecked. Run `angerona scan` in each.
- Anything that skips the hook: `--no-verify`, a git older than 2.54, a
  config that switches the hook off, or any tool that writes to the remote
  without git.

## Tests

```sh
cargo test && cargo build && bats test/
```

## License

MIT
