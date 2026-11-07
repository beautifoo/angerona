# angerona

Keeps the names on a private list, such as an employer, a client or a codename,
out of what you publish.

Angerona was the Roman goddess shown with her mouth sealed, who kept the secret
name of Rome.

It is one Rust binary. git runs it as a hook from your global config, in every
repo, and it refuses a commit whose message names a term on the list:

- `check-msg` reads the commit message, less its `#` comment lines and
  anything below `commit -v`'s scissors line. (With `commit -m` or `-F`,
  git keeps both, and so does `check-msg`.)

The list lives outside every repo, so the names it guards are never in one.

## Requirements

git 2.54 or later. Tested on macOS and Linux.

An older git skips config-based hooks without a word, so a commit made with
it skips the guard. Update it, with `brew install git` for example. A git an
app bundles is not on your PATH: point the app at a newer git instead.

## Install

Build it with cargo, then install a copy:

```sh
cargo build --release --locked
mkdir -p ~/.local/bin
install -m 755 target/release/angerona ~/.local/bin/angerona
```

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
`ace` does in `place`.

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
back, every commit is refused.

## Wire it into git

```sh
git config set --global hook.angerona-msg.event commit-msg
git config set --global hook.angerona-msg.command '~/.local/bin/angerona check-msg'
```

A repo's own hooks still run, after this one.

Where the names belong, in work repos for example, switch the hook off with
a config that your global config includes:

```
[includeIf "gitdir:~/work/"]
	path = config-work
```

and, in `config-work`:

```
[hook "angerona-msg"]
	enabled = false
```

A repo's own config (`.git/config`) can switch the hook off too.

## What it won't catch

It matches text, and only what passes through its hook:

- a term rephrased, abbreviated, misspelt or encoded, or with an invisible
  character inside it, such as a soft hyphen or a zero-width space;
- what skips the hook: `--no-verify`, a repo's own config switching it
  off, a git older than 2.54, anything that controls git's environment, the
  GitHub web UI, or any other tool that writes to the remote.

## Exit codes

0 passes. 1 refuses, and says where each hit is. 2 is a usage error.

## Tests

```sh
cargo test && cargo build && bats test/
```

The bats tests use a fake list, in throwaway repos.

## License

MIT
