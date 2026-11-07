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

# A bare "origin" holding one clean commit, and a clone of it as the cwd.
repo() {
	git init -q --bare "$BATS_TEST_TMPDIR/origin.git"
	git clone -q "$BATS_TEST_TMPDIR/origin.git" "$BATS_TEST_TMPDIR/clone" 2>/dev/null
	cd "$BATS_TEST_TMPDIR/clone"
	echo clean >README
	git add README
	git commit -qm "first"
	git push -q origin main
}

commit() { # FILE CONTENT [MESSAGE]
	printf '%s\n' "$2" >"$1"
	git add "$1"
	git commit -qm "${3:-change $1}"
}

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

@test "a term matches across a line break" {
	run "$AG" check <<<$'we sell Acme\nWidgets here'
	[ "$status" -eq 1 ]
	has "line 1: Acme ⏎ Widgets"
	repo
	printf 'we sell Acme\nWidgets here\n' >a
	git add a
	run "$AG" check-staged
	[ "$status" -eq 1 ]
	has "a:1: Acme ⏎ Widgets"
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

@test "a staged hit names its file and line" {
	repo
	commit a $'one\ntwo'
	printf 'one\ntwo\nthree globex\n' >a
	: >quux.txt
	git add a quux.txt
	run "$AG" check-staged
	[ "$status" -eq 1 ]
	has "a:3: globex"
	has "file quux.txt: quux"
}

# --- check-msg ---------------------------------------------------------------

@test "check-msg refuses a message naming a term" {
	printf 'fix: tidy\n\nfor the globex team\n' >"$BATS_TEST_TMPDIR/msg"
	run "$AG" check-msg "$BATS_TEST_TMPDIR/msg"
	[ "$status" -eq 1 ]
	has "message line 3: globex"
}

@test "check-msg honours core.commentChar and core.commentString" {
	repo
	printf 'fix: tidy\n; for globex\n# notes\n' >"$BATS_TEST_TMPDIR/msg"
	git config core.commentChar ";"
	run "$AG" check-msg "$BATS_TEST_TMPDIR/msg"
	[ "$status" -eq 0 ]
	git config core.commentString "//"
	printf 'fix: tidy\n// for globex\n; for globex\n' >"$BATS_TEST_TMPDIR/msg"
	run "$AG" check-msg "$BATS_TEST_TMPDIR/msg"
	[ "$status" -eq 1 ]
	has "message line 3: globex"
	lacks "message line 2"
}

@test "check-msg reads core.commentChar auto in any case as git does" {
	repo
	printf 'fix: tidy\nAUTO for globex\n' >"$BATS_TEST_TMPDIR/msg"
	git config core.commentChar AUTO
	run "$AG" check-msg "$BATS_TEST_TMPDIR/msg"
	[ "$status" -eq 1 ]
	has "message line 2: globex"
}

@test "check-msg finds the repo's comment string through GIT_DIR" {
	repo
	git config core.commentString ";"
	printf 'fix: tidy\n; for globex\n' >"$BATS_TEST_TMPDIR/msg"
	cd "$BATS_TEST_TMPDIR"
	GIT_DIR=$BATS_TEST_TMPDIR/clone/.git run "$AG" check-msg "$BATS_TEST_TMPDIR/msg"
	[ "$status" -eq 0 ]
}

@test "check-msg reads every line when git ran no editor, as git keeps them" {
	printf 'fix: tidy\n\n# notes for globex\n' >"$BATS_TEST_TMPDIR/msg"
	GIT_EDITOR=: run "$AG" check-msg "$BATS_TEST_TMPDIR/msg"
	[ "$status" -eq 1 ]
	has "message line 3: globex"
}

# --- check-staged ------------------------------------------------------------

@test "check-staged passes a clean change and refuses an added term" {
	repo
	echo fine >a
	git add a
	run "$AG" check-staged
	[ "$status" -eq 0 ]
	echo "for globex" >b
	git add b
	run "$AG" check-staged
	[ "$status" -eq 1 ]
	has "b:1: globex"
}

@test "check-staged ignores removed lines" {
	repo
	commit a "for globex"
	echo fine >a
	git add a
	run "$AG" check-staged
	[ "$status" -eq 0 ]
}

@test "check-staged refuses a term in a file name, even an empty file's" {
	repo
	: >globex.txt
	git add globex.txt
	run "$AG" check-staged
	[ "$status" -eq 1 ]
	has "file globex.txt: globex"
}

@test "check-staged can't be fooled by diff config" {
	repo
	git config color.ui always
	git config diff.noprefix true
	git config diff.external /usr/bin/true
	printf '*.txt diff=hide\n' >.gitattributes
	git config diff.hide.textconv /usr/bin/true
	echo "for globex" >b.txt
	git add .gitattributes b.txt
	run "$AG" check-staged
	[ "$status" -eq 1 ]
	has "b.txt:1: globex"
}

@test "check-staged scans a submodule entry's name, whatever diff.ignoreSubmodules says" {
	repo
	git config diff.ignoreSubmodules all
	git update-index --add --cacheinfo "160000,$(git rev-parse HEAD),globex"
	run "$AG" check-staged
	[ "$status" -eq 1 ]
	has "file globex: globex"
}

@test "check-staged reads the index git is about to commit" {
	repo
	cp .git/index "$BATS_TEST_TMPDIR/next-index"
	echo "for globex" >README
	GIT_INDEX_FILE=$BATS_TEST_TMPDIR/next-index git add README
	GIT_INDEX_FILE=$BATS_TEST_TMPDIR/next-index run "$AG" check-staged
	[ "$status" -eq 1 ]
	has "README:1: globex"
	run "$AG" check-staged
	[ "$status" -eq 0 ]
}

@test "check-staged ignores ceiling directories that don't cover the repo, as git does" {
	repo
	echo fine >a
	git add a
	GIT_CEILING_DIRECTORIES=/usr run "$AG" check-staged
	[ "$status" -eq 0 ]
	echo "for globex" >b
	git add b
	GIT_CEILING_DIRECTORIES=/usr run "$AG" check-staged
	[ "$status" -eq 1 ]
	has "b:1: globex"
}

@test "check-staged scans a symlink's target" {
	repo
	ln -s "globex-notes" link
	git add link
	run "$AG" check-staged
	[ "$status" -eq 1 ]
	has "link:1: globex"
}

@test "two file names never spell a term between them" {
	repo
	: >Acme
	: >Widgets
	git add Acme Widgets
	run "$AG" check-staged
	[ "$status" -eq 0 ]
}

# --- what is published, beyond lines of text --------------------------------

@test "check-staged refuses when the commit's identity names a term" {
	repo
	echo fine >a
	git add a
	GIT_AUTHOR_EMAIL=me@globex.example run "$AG" check-staged
	[ "$status" -eq 1 ]
	has "identity: globex"
}

@test "a file with no NUL in its first 8000 bytes is text, read in full" {
	repo
	{ head -c 8000 /dev/zero | tr '\000' 'x'; printf '\000 globex\n'; } >late.dat
	git add late.dat
	run "$AG" check-staged
	[ "$status" -eq 1 ]
	has "late.dat:1: globex"
}

@test "a UTF-16 file with a byte-order mark is text" {
	repo
	printf '\377\376f\000o\000r\000 \000g\000l\000o\000b\000e\000x\000\n\000' >u16.txt
	git add u16.txt
	run "$AG" check-staged
	[ "$status" -eq 1 ]
	has "u16.txt:1: globex"
}

@test "a byte-order mark on a file with no NUL leaves it text" {
	repo
	printf '\377\376for globex\n' >bom.txt
	git add bom.txt
	run "$AG" check-staged
	[ "$status" -eq 1 ]
	has "bom.txt:1: globex"
}

@test "check-staged passes removing a file whose name names a term" {
	repo
	commit globex.txt fine
	git rm -q globex.txt
	run "$AG" check-staged
	[ "$status" -eq 0 ]
}

@test "a term in a non-ASCII file name is found" {
	repo
	echo fine >"$(printf '\303\274')globex.txt"
	git add .
	run "$AG" check-staged
	[ "$status" -eq 1 ]
	has "globex.txt: globex"
}

@test "check-msg reads a file named -" {
	printf 'for globex\n' >"$BATS_TEST_TMPDIR/-"
	cd "$BATS_TEST_TMPDIR"
	run "$AG" check-msg - </dev/null
	[ "$status" -eq 1 ]
	has "message line 1: globex"
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

@test "check-msg skips the comments and diff git strips from a message" {
	printf 'remove the leaked file\n# Changes to be committed:\n#\tdeleted: globex.txt\n# ------------------------ >8 ------------------------\ndiff --git a/globex.txt b/globex.txt\n-for globex\n' >"$BATS_TEST_TMPDIR/msg"
	run "$AG" check-msg "$BATS_TEST_TMPDIR/msg"
	[ "$status" -eq 0 ]
}
