#!/usr/bin/env bats
# angerona against a fake term list and throwaway repos. Run: bats test/

setup() {
	AG=${AG:-$BATS_TEST_DIRNAME/../target/debug/angerona}
	export XDG_CONFIG_HOME=$BATS_TEST_TMPDIR/config HOME=$BATS_TEST_TMPDIR/home
	terms=$XDG_CONFIG_HOME/angerona/terms.txt
	mkdir -p "$XDG_CONFIG_HOME/angerona" "$HOME"
	printf '%s\n' \
		'# fake terms, made up for these tests' \
		zorblax quux FROBCORP globex 'Acme Widgets' >"$terms"
	# Keep the user's git config (signing, aliases, colours) out of the tests.
	export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=$BATS_TEST_TMPDIR/gitconfig
	git config --global user.name Tester
	git config --global user.email tester@example.com
	git config --global init.defaultBranch main
	export TMPDIR=$BATS_TEST_TMPDIR/tmp
	mkdir "$TMPDIR"
}

teardown() {
	# angerona leaves nothing in TMPDIR.
	[ -z "$(ls -A "$TMPDIR")" ]
}

# Assertions on $output. bash 3.2 lets a failing [[ ]] through unless it is a
# test's last command, so these are plain commands.
has() { [[ $output == *"$1"* ]] || { echo "output lacks: $1"; return 1; }; }
lacks() { [[ $output != *"$1"* ]] || { echo "output has: $1"; return 1; }; }

# --- the term list -----------------------------------------------------------

@test "check passes text that names no term" {
	run "$AG" check <<<"nothing to see here"
	[ "$status" -eq 0 ]
}

@test "a term matches inside a longer word, reporting only itself" {
	for text in globex globexing myglobex xglobexy; do
		run "$AG" check <<<"at $text today"
		[ "$status" -eq 1 ] || { echo "passed: $text"; return 1; }
	done
	run "$AG" check <<<"the xglobexy plan"
	has "line 1: globex"
	lacks "xglobex"
}

@test "a term matches in any case" {
	run "$AG" check <<<"at GloBex today"
	[ "$status" -eq 1 ]
	run "$AG" check <<<"frobcorp too"
	[ "$status" -eq 1 ]
}

@test "a term matches with any separators, or none, between its words" {
	for text in Acme-Widgets acme_widgets AcmeWidgets "acme .. widgets"; do
		run "$AG" check <<<"from $text today"
		[ "$status" -eq 1 ] || { echo "passed: $text"; return 1; }
	done
	run "$AG" check <<<"acme gadgets and widgets"
	[ "$status" -eq 0 ]
}

@test "a term matches across a blank line or several lines" {
	run "$AG" check <<<$'we sell Acme\n\n--\nWidgets here'
	[ "$status" -eq 1 ]
	has "line 1: Acme ⏎  ⏎ -- ⏎ Widgets"
}

@test "letters are Unicode letters, in any case" {
	printf 'Zürich\n' >"$terms"
	run "$AG" check <<<"to ZÜRICH"
	[ "$status" -eq 1 ]
	run "$AG" check <<<"to Z-rich"
	[ "$status" -eq 0 ]
	printf 'Москва\n' >"$terms"
	run "$AG" check <<<"в москва"
	[ "$status" -eq 1 ]
}

@test "regex characters in a term are only separators" {
	printf 'qu+ux\n' >"$terms"
	for text in quux qu+ux "qu ux"; do
		run "$AG" check <<<"x $text y"
		[ "$status" -eq 1 ] || { echo "passed: $text"; return 1; }
	done
	run "$AG" check <<<"quuux"
	[ "$status" -eq 0 ]
}

@test "comment lines, indented or not, are skipped" {
	printf 'zorblax\n  # a note\n#another\n' >"$terms"
	run "$AG" check <<<"a note, another"
	[ "$status" -eq 0 ]
}

@test "a line with no letters or digits refuses everything" {
	printf 'zorblax\n--\n' >"$terms"
	run "$AG" check <<<"harmless"
	[ "$status" -eq 1 ]
	has "line 2"
}

@test "a line with a tab refuses everything, naming its line and not its text" {
	printf 'zorblax\nterm\tsecretword\n' >"$terms"
	run "$AG" check <<<"harmless"
	[ "$status" -eq 1 ]
	has "line 2"
	lacks secretword
}

@test "the list defaults to ~/.config/angerona/terms.txt" {
	mkdir -p "$BATS_TEST_TMPDIR/home/.config/angerona"
	cp "$terms" "$BATS_TEST_TMPDIR/home/.config/angerona/terms.txt"
	HOME=$BATS_TEST_TMPDIR/home XDG_CONFIG_HOME= run "$AG" check <<<"globex"
	[ "$status" -eq 1 ]
	has "line 1: globex"
}

@test "a missing list refuses everything" {
	XDG_CONFIG_HOME=$BATS_TEST_TMPDIR/nowhere run "$AG" check <<<"harmless"
	[ "$status" -eq 1 ]
	has "refusing to pass anything"
}

@test "the list is never read from the working directory" {
	mkdir -p "$BATS_TEST_TMPDIR/cwd/.config/angerona"
	printf 'zzzqqq\n' >"$BATS_TEST_TMPDIR/cwd/.config/angerona/terms.txt"
	cd "$BATS_TEST_TMPDIR/cwd"
	HOME= XDG_CONFIG_HOME= run "$AG" check <<<"zzzqqq"
	[ "$status" -eq 1 ]
	has "can't read"
	run env -u HOME -u XDG_CONFIG_HOME "$AG" check <<<"zzzqqq"
	[ "$status" -eq 1 ]
	has "can't read"
	HOME=. XDG_CONFIG_HOME=.config run "$AG" check <<<"zzzqqq"
	[ "$status" -eq 1 ]
	has "can't read"
}

@test "a list with no term refuses everything" {
	printf '# only a comment\n\n   \n' >"$terms"
	run "$AG" check <<<"harmless"
	[ "$status" -eq 1 ]
	has "no term in"
}

# --- what a refusal says -----------------------------------------------------

@test "a hit names its line" {
	run "$AG" check <<<$'one\ntwo zorblax'
	[ "$status" -eq 1 ]
	has "line 2: zorblax"
}

@test "a list saved with CRLF line ends still guards" {
	printf 'zorblax\r\n# comment\r\n\r\nglobex\r\n' >"$terms"
	run "$AG" check <<<"the zorblax plan"
	[ "$status" -eq 1 ]
	run "$AG" check <<<"harmless"
	[ "$status" -eq 0 ]
}

@test "a list that would merge terms refuses everything" {
	printf 'zorblax\rglobex\r' >"$terms"
	run "$AG" check <<<"harmless"
	[ "$status" -eq 1 ]
	printf 'zorblax # a note\n' >"$terms"
	run "$AG" check <<<"harmless"
	[ "$status" -eq 1 ]
	has "line 1"
}

# --- edge cases --------------------------------------------------------------

@test "a term matches next to an underscore" {
	for text in GLOBEX_API_KEY=1 my_globex_client acme_widgets_v2; do
		run "$AG" check <<<"$text"
		[ "$status" -eq 1 ] || { echo "passed: $text"; return 1; }
	done
	has "line 1: acme_widgets"
	run "$AG" check <<<$'one\nglobex_x'
	has "line 2: globex"
	lacks "⏎"
}

@test "a term ending in punctuation matches before another word" {
	printf 'Globex Corp.\n' >"$terms"
	run "$AG" check <<<"Globex Corp. said hi"
	[ "$status" -eq 1 ]
	has "line 1: Globex Corp"
}
