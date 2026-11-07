# angerona

Keeps the names on a private list, such as an employer, a client or a codename,
out of what you publish.

Angerona was the Roman goddess shown with her mouth sealed, who kept the secret
name of Rome.

It is one Rust binary, and it refuses text that names a term on the list.

The list lives outside every repo, so the names it guards are never in one.

## Requirements

Tested on macOS and Linux.

## Install

Build it with cargo, then install a copy:

```sh
cargo build --release --locked
mkdir -p ~/.local/bin
install -m 755 target/release/angerona ~/.local/bin/angerona
```

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

Keep a private backup, in a password manager for example.

## What it won't catch

It matches text:

- a term rephrased, abbreviated, misspelt or encoded, or with an invisible
  character inside it, such as a soft hyphen or a zero-width space.

## Exit codes

0 passes. 1 refuses, and says where each hit is. 2 is a usage error.

## Tests

```sh
cargo test && cargo build && bats test/
```

The bats tests use a fake list.

## License

MIT
