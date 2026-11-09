# angerona

Keeps the names on a private list, such as an employer, a client or a codename,
out of everything you publish with git, and out of what an AI agent sends to
an MCP server.

Angerona was the Roman goddess shown with her mouth sealed, who kept the secret
name of Rome.

It is one Rust binary. git runs it as hooks from your global config, in every
repo, and it refuses a commit or push that names a term on the list:

- `check-msg` reads the commit message, less its `#` comment lines and
  anything below `commit -v`'s scissors line. (With `commit -m` or `-F`,
  git keeps both, and so does `check-msg`.)
- `check-staged` reads each staged file in full, each file name, and the
  author the commit will carry; `check-push` reads the committer.
- `check-push` reads every object the push sends that the remote doesn't
  hold yet: each commit and tag as stored, every header and identity
  included, but not a commit's signature, whose random base64 can spell a
  short term by chance; each file version in full; and every file and ref
  name. It asks the remote what it holds; when it can't, it scans each
  pushed ref's whole history.

A binary file, one with a NUL byte in its first 8000 bytes as git reckons
it, is read only for its runs of 16 or more readable characters. That keeps
the text a program wrote into it, such as an image's XMP or a PDF's
properties, and leaves out its compressed bytes, where a short term would
turn up by chance. A file that starts with a UTF-16 byte-order mark is read
as text.

A Claude Code hook runs it over every MCP call too (`check-json`).

The list lives outside every repo, so the names it guards are never in one.

## Requirements

git 2.54 or later. Tested on macOS and Linux.

An older git skips config-based hooks without a word, so a commit or push
made with it skips the guard. `angerona doctor` names each git on your PATH
that would; update it, with `brew install git` for example. A git an app
bundles is not on your PATH: point the app at a newer git instead.

## Install

From the release archive, once you have checked it was built from the commit
you reviewed:

```sh
gh release download --repo beautifoo/angerona --pattern angerona-aarch64-apple-darwin.tar.xz
gh attestation verify angerona-aarch64-apple-darwin.tar.xz --repo beautifoo/angerona --source-digest <the commit you reviewed>
tar -xJf angerona-aarch64-apple-darwin.tar.xz
mkdir -p ~/.local/bin
install -m 755 angerona-aarch64-apple-darwin/angerona ~/.local/bin/angerona
```

Or with Homebrew:

```sh
brew install beautifoo/tap/angerona
```

The commands below use `~/.local/bin/angerona`; after a Homebrew install, use
the full path `$(brew --prefix)/bin/angerona` expands to instead, such as
`/opt/homebrew/bin/angerona`. Only an `Edit(...)` rule spells an absolute path
differently, with `//`: `Edit(//opt/homebrew/bin/angerona)`.

Install a copy rather than pointing git at a checkout: then editing the
checkout, yours or an agent's, doesn't change the guard until you install
again.

## The list

`~/.config/angerona/terms.txt` (`$XDG_CONFIG_HOME/angerona/terms.txt` when
that is set): one term per line. Blank lines, and lines whose first character
other than a space is `#`, are skipped; a comment needs a line of its own.

```
# names that stay out of what I publish
Initech
Acme Widgets
globex.example
```

A term matches anywhere, even inside a longer word, in any case, with
anything but letters and digits, or nothing, between its words:
`Acme Widgets` matches `acme-widgets`, `MyAcmeWidgetsApp`,
`ACME_WIDGETS_KEY`, and `Acme` at the end of one line with `Widgets` at the
start of the next. Punctuation only ever separates: `globex.example` matches
`globex example` too, and `"Initech"` is plain `Initech`. Letters and
digits are Unicode ones, and case folds across scripts, so `Zürich` matches
`ZÜRICH` but not `Zurich`. List each spelling you want caught.

Keep terms distinctive. A short one also matches inside everyday words, as
`ace` does in `place`, and one made only of digits and the letters a to f
can turn up inside the commit hashes a push is scanned with.

Without the list, with no term in it, or with a line it can't read (one with
no letter or digit, a tab, a carriage return inside it, or a `#` after a
space), every check refuses everything, and says which line is wrong without
printing it.

```sh
mkdir -m 700 -p ~/.config/angerona
$EDITOR ~/.config/angerona/terms.txt
chmod 600 ~/.config/angerona/terms.txt
```

Keep a private backup, in a password manager for example: until the list is
back, every commit and push is refused.

## Wire it into git

```sh
git config set --global hook.angerona-msg.event commit-msg
git config set --global hook.angerona-msg.command '~/.local/bin/angerona check-msg'
git config set --global hook.angerona-staged.event pre-commit
git config set --global hook.angerona-staged.command '~/.local/bin/angerona check-staged'
git config set --global hook.angerona-push.event pre-push
git config set --global hook.angerona-push.command '~/.local/bin/angerona check-push'
~/.local/bin/angerona doctor
```

`doctor` checks the hooks, the list and each git on your PATH from the repo
you run it in, and prints the command that fixes each problem. A repo's own
hooks still run, after these.

Where the names belong, in work repos for example, switch the hooks off with
a config that your global config includes:

```
[includeIf "gitdir:~/work/"]
	path = config-work
```

and, in `config-work`:

```
[hook "angerona-msg"]
	enabled = false
[hook "angerona-staged"]
	enabled = false
[hook "angerona-push"]
	enabled = false
```

In such a repo, `doctor` says the guard is switched off, and by which file.
A repo's own config (`.git/config`) can switch the hooks off too, but
`doctor` flags that, since an agent can write it.

## Claude Code

Scan every MCP call, in `~/.claude/settings.json`, running the binary by its
full path, with no shell. `check-json` exits 2 on a refusal and on every
error, which blocks the call:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "mcp__.*",
        "hooks": [
          {
            "type": "command",
            "command": "/Users/you/.local/bin/angerona",
            "args": ["check-json"],
            "onFailure": "block"
          }
        ]
      }
    ]
  }
}
```

A session's calls follow its project's git: where git has `angerona-push`
switched off, by the `config-work` include above for example, the session is
a work session. In one, a GitHub call passes only when it names an owner or
organization and every one it names is an owner of the remote the current
branch pushes to (git's push remote: `branch.<name>.pushRemote`,
`remote.pushDefault`, the branch's remote, `origin`), and is scanned
otherwise, a call that names none included. Every other MCP tool passes, so
browser tools (Playwright and the like) can post anywhere unchecked there.
The shell is never checked, in a work session or not. A repo's own
`.git/config` can start a work session too. The project is the directory the
session started in, so a session started outside a git repo is always
checked.

A hook only sees what passes through it. An agent with a shell can reach
GitHub without the MCP server (`gh`, `curl`, `git push --no-verify`), or
edit the guard. Where that matters, take those away in the project's
`.claude/settings.json`: no GitHub network for the shell, and no writes to
the guard, its list, or the global git config and the Claude config. That
includes this file and `.claude/settings.local.json`: either could set
`"disableAllHooks": true`, or drop this block. Claude Code's sandbox already
keeps the shell from writing them, but in auto mode a classifier can approve
an edit to them, which the deny rules below rule out. A repo's own
`.git/config` stays writable: `doctor` flags a switch-off there, as above.

```json
{
  "sandbox": {
    "enabled": true,
    "failIfUnavailable": true,
    "allowUnsandboxedCommands": false,
    "network": {
      "deniedDomains": ["github.com", "*.github.com", "*.githubusercontent.com"]
    },
    "filesystem": {
      "denyWrite": [
        "~/.local/bin/angerona",
        "~/.config/angerona",
        "~/.config/git",
        "~/.gitconfig",
        "~/.claude/settings.json"
      ]
    }
  },
  "permissions": {
    "deny": [
      "Edit(~/.local/bin/angerona)",
      "Edit(~/.config/angerona/**)",
      "Edit(~/.config/git/**)",
      "Edit(~/.gitconfig)",
      "Edit(~/.claude/settings.json)",
      "Edit(/.claude/settings.json)",
      "Edit(/.claude/settings.local.json)"
    ]
  }
}
```

## What it won't catch

It matches text, and only what passes through its hooks:

- a term rephrased, abbreviated, misspelt or encoded, or with an invisible
  character inside it, such as a soft hyphen or a zero-width space;
- a term in a binary file that isn't in a run of 16 or more readable
  characters: a short metadata field, text in a zip, an image's pixels, or
  most of a PDF's pages;
- what skips the hooks: `--no-verify`, a repo's own config switching them
  off, a git older than 2.54, push options (`git push -o`), anything that
  controls git's environment, the GitHub web UI, or any other tool that
  writes to the remote.

## Exit codes

0 passes. 1 refuses, and says where each hit is: `path:line: match`,
`commit <sha>: match`, `ref <name>: match`. 2 is a usage error. `check-json`
exits 2 on a refusal too, which blocks the call.

## Tests

```sh
cargo test && cargo build && bats test/
```

The bats tests use a fake list, in throwaway repos.

## License

MIT
