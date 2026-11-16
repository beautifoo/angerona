#!/usr/bin/env bats
# angerona against a fake term list and throwaway repos. Run: bats test/

zero=0000000000000000000000000000000000000000

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

# The line git gives pre-push for pushing REV to REF.
push_line() { printf '%s %s %s %s\n' "$2" "$(git rev-parse "$1")" "$2" "$zero"; }

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

@test "a term matches only as written, reported as listed" {
	run "$AG" check <<<"from xACME WIDGETSy today"
	[ "$status" -eq 1 ]
	has "line 1: Acme Widgets"
	for text in Acme-Widgets acme_widgets AcmeWidgets "acme  widgets" $'Acme\nWidgets'; do
		run "$AG" check <<<"from $text today"
		[ "$status" -eq 0 ] || { echo "refused: $text"; return 1; }
	done
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

@test "a term's punctuation matches only itself" {
	printf 'qu+ux\n' >"$terms"
	run "$AG" check <<<"x qu+ux y"
	[ "$status" -eq 1 ]
	for text in quux quuux "qu ux"; do
		run "$AG" check <<<"x $text y"
		[ "$status" -eq 0 ] || { echo "refused: $text"; return 1; }
	done
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

@test "a pushed hit names the ref, tag or commit it is in" {
	repo
	commit a "for globex"
	git commit -q --allow-empty -m $'neutral\n\nabout quux'
	git tag -a -m "zorblax" t1
	run "$AG" check-push origin < <(push_line t1 refs/tags/t1; push_line HEAD refs/heads/FROBCORP)
	[ "$status" -eq 1 ]
	has "ref refs/heads/FROBCORP: FROBCORP"
	has "tag $(git rev-parse t1): zorblax"
	has "a:1: globex"
	has "commit $(git rev-parse HEAD): quux"
}

@test "check-push refuses a commit whose author names a term" {
	repo
	git commit -q --allow-empty -m neutral --author="Someone <someone@globex.example>"
	run "$AG" check-push origin < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 1 ]
	has "commit $(git rev-parse HEAD): globex"
}

@test "usage errors exit 2" {
	run "$AG"
	[ "$status" -eq 2 ]
	run "$AG" check-msg
	[ "$status" -eq 2 ]
	run "$AG" check-push
	[ "$status" -eq 2 ]
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

# --- check-push --------------------------------------------------------------

@test "check-push passes a clean branch" {
	repo
	commit a fine
	run "$AG" check-push origin < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 0 ]
}

@test "check-push refuses a term in a ref name" {
	repo
	run "$AG" check-push origin < <(push_line HEAD refs/heads/globex)
	[ "$status" -eq 1 ]
	has "ref refs/heads/globex: globex"
}

@test "check-push refuses a term in a commit message" {
	repo
	git commit -q --allow-empty -m "for globex"
	run "$AG" check-push origin < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 1 ]
	has "commit $(git rev-parse HEAD): globex"
}

@test "check-push refuses a term in an added line" {
	repo
	commit a "for globex"
	run "$AG" check-push origin < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 1 ]
	has "a:1: globex"
}

@test "check-push refuses a term in a tag that another tag points at" {
	repo
	git tag -a -m "for globex" inner
	git tag -a -m "neutral" outer inner
	run "$AG" check-push origin < <(push_line outer refs/tags/outer)
	[ "$status" -eq 1 ]
	has "tag $(git rev-parse inner): globex"
}

@test "check-push passes a deletion" {
	repo
	run "$AG" check-push origin <<<"(delete) $zero refs/heads/globex $(git rev-parse HEAD)"
	[ "$status" -eq 0 ]
}

@test "check-push leaves out what the remote already holds" {
	repo
	commit a "for globex"
	git push -q --no-verify origin main
	git commit -q --allow-empty -m "neutral"
	run "$AG" check-push origin < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 0 ]
}

@test "check-push to a URL that holds nothing scans the whole history" {
	repo
	commit a "for globex"
	git push -q --no-verify origin main
	git init -q --bare "$BATS_TEST_TMPDIR/empty.git"
	run "$AG" check-push "$BATS_TEST_TMPDIR/empty.git" < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 1 ]
	has "a:1: globex"
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

@test "a binary file is scanned for its runs of readable text, not refused" {
	repo
	printf 'PNG\000\001data\000' >pic.png
	git add pic.png
	run "$AG" check-staged
	[ "$status" -eq 0 ]
	printf 'PNG\000\001<dc:creator>the globex team</dc:creator>\000' >pic.png
	git add pic.png
	run "$AG" check-staged
	[ "$status" -eq 1 ]
	has "binary pic.png: globex"
	git commit -qm "a picture"
	run "$AG" check-push origin < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 1 ]
	has "binary pic.png: globex"
}

@test "a term in a binary file's short runs, as random bytes spell one, passes" {
	repo
	printf 'PNG\000\001\217xGlObExq\377\000zorblax\000' >pic.png
	git add pic.png
	run "$AG" check-staged
	[ "$status" -eq 0 ]
	git commit -qm "a picture"
	run "$AG" check-push origin < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 0 ]
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

@test "check-push refuses a term in a file under a tag on a tree" {
	repo
	commit a "for globex"
	git tag -a -m neutral treetag "HEAD^{tree}"
	git reset -q --hard HEAD~1
	run "$AG" check-push origin < <(push_line treetag refs/tags/treetag)
	[ "$status" -eq 1 ]
	has "a:1: globex"
}

@test "check-push refuses a term in a commit header git log never shows" {
	repo
	tree=$(git rev-parse 'HEAD^{tree}')
	sha=$(printf 'tree %s\nparent %s\nauthor A <a@b> 1 +0000\ncommitter A <a@b> 1 +0000\nx-note globex\n\nneutral\n' \
		"$tree" "$(git rev-parse HEAD)" | git hash-object -t commit -w --stdin)
	run "$AG" check-push origin <<<"refs/heads/main $sha refs/heads/main $zero"
	[ "$status" -eq 1 ]
	has "commit $sha: globex"
}

# A commit object from HEADERS (printf format) and MESSAGE, on top of HEAD.
raw_commit() {
	printf "tree %s\nparent %s\nauthor A <a@b> 1 +0000\ncommitter A <a@b> 1 +0000\n$1\n%s\n" \
		"$(git rev-parse 'HEAD^{tree}')" "$(git rev-parse HEAD)" "$2" | git hash-object -t commit -w --stdin
}

@test "check-push skips a commit's signature, whose base64 can spell a term by chance" {
	repo
	for h in gpgsig gpgsig-sha256; do
		sha=$(raw_commit "$h -----BEGIN PGP SIGNATURE-----\n \n iQEzBAABCAAdglobexFiEE+x/Y\n -----END PGP SIGNATURE-----\n" neutral)
		run "$AG" check-push origin <<<"refs/heads/main $sha refs/heads/main $zero"
		[ "$status" -eq 0 ] || { echo "refused: $h"; return 1; }
	done
}

@test "check-push still reads the headers and message after a signature" {
	repo
	sha=$(raw_commit "gpgsig -----BEGIN PGP SIGNATURE-----\n iQEz\n -----END PGP SIGNATURE-----\nx-note globex\n" neutral)
	run "$AG" check-push origin <<<"refs/heads/main $sha refs/heads/main $zero"
	[ "$status" -eq 1 ]
	sha=$(raw_commit "gpgsig -----BEGIN PGP SIGNATURE-----\n iQEz\n -----END PGP SIGNATURE-----\n" $'neutral\n\ngpgsig globex\n iQEz globex')
	run "$AG" check-push origin <<<"refs/heads/main $sha refs/heads/main $zero"
	[ "$status" -eq 1 ]
}

@test "check-push reads the plain-text lines inside a signature" {
	repo
	sha=$(raw_commit "gpgsig -----BEGIN PGP SIGNATURE-----\n Comment: globex code signing\n \n iQEz\n -----END PGP SIGNATURE-----\n" neutral)
	run "$AG" check-push origin <<<"refs/heads/main $sha refs/heads/main $zero"
	[ "$status" -eq 1 ]
	has "commit $sha: globex"
}

@test "a NUL byte doesn't hide what follows it" {
	run "$AG" check <<<$'x\001Acme Widgets'
	[ "$status" -eq 1 ]
	printf 'x\000Acme Widgets\n' >"$BATS_TEST_TMPDIR/nul"
	run "$AG" check <"$BATS_TEST_TMPDIR/nul"
	[ "$status" -eq 1 ]
	repo
	printf 'release\000 globex\n' >"$BATS_TEST_TMPDIR/tagmsg"
	git tag --no-sign -a -F "$BATS_TEST_TMPDIR/tagmsg" v1
	run "$AG" check-push origin < <(push_line v1 refs/tags/v1)
	[ "$status" -eq 1 ]
}

@test "check-push takes a ref line as a deletion only by its zero object name" {
	repo
	commit a "for globex"
	run "$AG" check-push origin <<<"(delete) $(git rev-parse HEAD) refs/heads/x $zero"
	[ "$status" -eq 1 ]
}

@test "check-push reads the objects a push sends, not their replacements" {
	repo
	commit a "for globex"
	bad=$(git rev-parse HEAD)
	git reset -q --hard HEAD~1
	commit a fine
	git replace "$bad" HEAD
	run "$AG" check-push origin < <(push_line "$bad" refs/heads/main)
	[ "$status" -eq 1 ]
	has "a:1: globex"
}

@test "check-push scans what a merge brings in" {
	repo
	git checkout -q -b side
	commit b "for globex"
	git checkout -q main
	git merge -q --no-ff --no-edit side
	run "$AG" check-push origin < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 1 ]
	has "b:1: globex"
}

@test "check-push to a path ignores stale refs left under its name" {
	repo
	commit a "for globex"
	git push -q --no-verify origin main
	git update-ref refs/remotes/backup/main HEAD
	git init -q --bare backup
	run "$AG" check-push backup < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 1 ]
	has "a:1: globex"
}

@test "check-push reads a ref line whose source has a space" {
	repo
	commit a "for globex"
	run "$AG" check-push origin <<<"HEAD@{0 seconds ago} $(git rev-parse HEAD) refs/heads/main $zero"
	[ "$status" -eq 1 ]
	has "a:1: globex"
}

@test "check-msg reads a file named -" {
	printf 'for globex\n' >"$BATS_TEST_TMPDIR/-"
	cd "$BATS_TEST_TMPDIR"
	run "$AG" check-msg - </dev/null
	[ "$status" -eq 1 ]
	has "message line 1: globex"
}

# --- check-json, for a Claude Code PreToolUse hook ---------------------------

@test "check-json scans every string in a tool call's input" {
	run "$AG" check-json <<<'{"tool_name":"mcp__github__create_pull_request","tool_input":{"title":"fine","body":"line one\nfor globex","labels":["ok"]}}'
	[ "$status" -eq 2 ]
	has "body:2: globex"
	run "$AG" check-json <<<'{"tool_name":"mcp__github__get_me","tool_input":{"files":[{"path":"a","content":"fine"}]}}'
	[ "$status" -eq 0 ]
}

@test "check-json refuses input it can't parse" {
	run "$AG" check-json <<<'{not json'
	[ "$status" -eq 2 ]
	has "can't read the tool call"
}

@test "check-json passes every call in a project where git has the guard switched off" {
	repo
	hooks
	printf '[hook "angerona-push"]\n\tenabled = false\n' >"$BATS_TEST_TMPDIR/work.gitconfig"
	git config --global "includeIf.gitdir:$BATS_TEST_TMPDIR/clone/.path" "$BATS_TEST_TMPDIR/work.gitconfig"
	input='{"tool_input":{"body":"for globex"}}'
	CLAUDE_PROJECT_DIR=$BATS_TEST_TMPDIR/clone run "$AG" check-json <<<"$input"
	[ "$status" -eq 0 ]
	XDG_CONFIG_HOME=$BATS_TEST_TMPDIR/nowhere CLAUDE_PROJECT_DIR=$BATS_TEST_TMPDIR/clone run "$AG" check-json <<<"$input"
	[ "$status" -eq 0 ]
	git init -q "$BATS_TEST_TMPDIR/other"
	CLAUDE_PROJECT_DIR=$BATS_TEST_TMPDIR/other run "$AG" check-json <<<"$input"
	[ "$status" -eq 2 ]
	CLAUDE_PROJECT_DIR= run "$AG" check-json <<<"$input"
	[ "$status" -eq 2 ]
	# The repo's own config switches it back on.
	git config hook.angerona-push.enabled true
	CLAUDE_PROJECT_DIR=$BATS_TEST_TMPDIR/clone run "$AG" check-json <<<"$input"
	[ "$status" -eq 2 ]
	has "body:1: globex"
}

@test "check-json follows a switch-off in the repo's own config, as git does" {
	repo
	hooks
	git config hook.angerona-push.enabled false
	CLAUDE_PROJECT_DIR=$BATS_TEST_TMPDIR/clone run "$AG" check-json <<<'{"tool_input":{"body":"for globex"}}'
	[ "$status" -eq 0 ]
}

@test "check-json reads the push hook's switch as git's hooks do, not as git config does" {
	repo
	hooks
	input='{"tool_input":{"body":"for globex"}}'
	printf '[hook "angerona-push"]\n\tenabled = false\n' >"$BATS_TEST_TMPDIR/off.gitconfig"
	GIT_CONFIG=$BATS_TEST_TMPDIR/off.gitconfig CLAUDE_PROJECT_DIR=$BATS_TEST_TMPDIR/clone run "$AG" check-json <<<"$input"
	[ "$status" -eq 2 ]
	git config --global hook.angerona-push.enabled false
	git config hook.angerona-push.enabled maybe
	CLAUDE_PROJECT_DIR=$BATS_TEST_TMPDIR/clone run "$AG" check-json <<<"$input"
	[ "$status" -eq 0 ]
}

@test "check-json passes where git switches off every pre-push hook" {
	repo
	hooks
	git config hook.pre-push.enabled false
	CLAUDE_PROJECT_DIR=$BATS_TEST_TMPDIR/clone run "$AG" check-json <<<'{"tool_input":{"body":"for globex"}}'
	[ "$status" -eq 0 ]
}

@test "check-json, in a work session, passes GitHub calls only to the repo's own owners" {
	repo
	hooks
	git remote add gh git@github.com:workorg/app.git
	git config remote.pushDefault gh
	git config hook.angerona-push.enabled false
	export CLAUDE_PROJECT_DIR=$BATS_TEST_TMPDIR/clone
	run "$AG" check-json <<<'{"tool_name":"mcp__github__create_issue","tool_input":{"owner":"workorg","body":"for globex"}}'
	[ "$status" -eq 0 ]
	run "$AG" check-json <<<'{"tool_name":"mcp__github__create_issue","tool_input":{"owner":"me","body":"for globex"}}'
	[ "$status" -eq 2 ]
	run "$AG" check-json <<<'{"tool_name":"mcp__github__search_code","tool_input":{"query":"globex"}}'
	[ "$status" -eq 2 ]
	run "$AG" check-json <<<'{"tool_name":"mcp__plugin_slack_slack__slack_send_message","tool_input":{"message":"for globex"}}'
	[ "$status" -eq 0 ]
}

@test "check-json, in a work session, counts only the remote the branch pushes to" {
	repo
	hooks
	git config hook.angerona-push.enabled false
	git remote set-url origin git@github.com:workorg/app.git
	git remote add upstream git@github.com:ossorg/lib.git
	export CLAUDE_PROJECT_DIR=$BATS_TEST_TMPDIR/clone
	run "$AG" check-json <<<'{"tool_name":"mcp__github__create_issue","tool_input":{"owner":"ossorg","body":"for globex"}}'
	[ "$status" -eq 2 ]
	has "body:1: globex"
	run "$AG" check-json <<<'{"tool_name":"mcp__github__create_issue","tool_input":{"owner":"workorg","body":"for globex"}}'
	[ "$status" -eq 0 ]
	git config branch.main.pushRemote upstream
	run "$AG" check-json <<<'{"tool_name":"mcp__github__create_issue","tool_input":{"owner":"ossorg","body":"for globex"}}'
	[ "$status" -eq 0 ]
	run "$AG" check-json <<<'{"tool_name":"mcp__github__create_issue","tool_input":{"owner":"workorg","body":"for globex"}}'
	[ "$status" -eq 2 ]
}

@test "check-json scans every call when ceiling directories are set" {
	repo
	hooks
	git config hook.angerona-push.enabled false
	GIT_CEILING_DIRECTORIES=$BATS_TEST_TMPDIR CLAUDE_PROJECT_DIR=$BATS_TEST_TMPDIR/clone run "$AG" check-json <<<'{"tool_input":{"body":"for globex"}}'
	[ "$status" -eq 2 ]
}

@test "check-json reads files named in a call, not directories or missing paths" {
	echo "for globex" >"$BATS_TEST_TMPDIR/page.html"
	run "$AG" check-json <<<"{\"tool_input\":{\"file_path\":\"$BATS_TEST_TMPDIR/page.html\"}}"
	[ "$status" -eq 2 ]
	has "$BATS_TEST_TMPDIR/page.html:1: globex"
	run "$AG" check-json <<<"{\"tool_input\":{\"a\":\"/\",\"b\":\"$BATS_TEST_TMPDIR\",\"c\":\"$BATS_TEST_TMPDIR/not-yet.png\"}}"
	[ "$status" -eq 0 ]
}

@test "check-json refuses a call with no tool_input" {
	run "$AG" check-json <<<'{"tool_name":"x","toolArgs":{"body":"fine"}}'
	[ "$status" -eq 2 ]
}

# --- wired as git's config hooks, as in the global git config ----------------

config_hooks() { git hook list -h 2>&1 | grep -q -- --show-scope || skip "this git has no config hooks"; }

hooks() {
	config_hooks
	git config --global hook.angerona-msg.event commit-msg
	git config --global hook.angerona-msg.command "$AG check-msg"
	git config --global hook.angerona-staged.event pre-commit
	git config --global hook.angerona-staged.command "$AG check-staged"
	git config --global hook.angerona-push.event pre-push
	git config --global hook.angerona-push.command "$AG check-push"
}

@test "git's config hooks refuse a commit or push that names a term" {
	repo
	hooks
	echo "for globex" >a
	git add a
	run git commit -qm "neutral"
	[ "$status" -ne 0 ]
	echo fine >a
	git add a
	run git commit -qm "for globex"
	[ "$status" -ne 0 ]
	git commit -qm "neutral"
	git checkout -q -b globex
	run git push -q origin globex
	[ "$status" -ne 0 ]
	run git push -q origin HEAD:main
	[ "$status" -eq 0 ]
}

@test "git's config hooks can be switched off for a directory" {
	repo
	hooks
	printf '[hook "angerona-staged"]\n\tenabled = false\n' >"$BATS_TEST_TMPDIR/work.gitconfig"
	git config --global "includeIf.gitdir:$BATS_TEST_TMPDIR/clone/.path" "$BATS_TEST_TMPDIR/work.gitconfig"
	echo "for globex" >a
	git add a
	git commit -qm "neutral"
}

# --- doctor ------------------------------------------------------------------

@test "doctor passes when the hooks are wired and the list is in place" {
	repo
	hooks
	chmod 600 "$terms"
	run "$AG" doctor
	[ "$status" -eq 0 ]
}

@test "doctor names each problem with the command that fixes it" {
	repo
	config_hooks
	chmod 644 "$terms"
	run "$AG" doctor
	[ "$status" -eq 1 ]
	has "no angerona-msg hook for commit-msg"
	has "git config set --global hook.angerona-push.command '$(cd "$(dirname "$AG")" && pwd)/$(basename "$AG") check-push'"
	has "chmod 600 $terms"
	XDG_CONFIG_HOME=$BATS_TEST_TMPDIR/nowhere run "$AG" doctor
	[ "$status" -eq 1 ]
	has "can't read $BATS_TEST_TMPDIR/nowhere/angerona/terms.txt"
}

# A git in $BATS_TEST_TMPDIR/oldgit that answers --version with ARGS and
# hands anything else to the real git.
oldgit() {
	mkdir -p "$BATS_TEST_TMPDIR/oldgit"
	printf '#!/bin/sh\n[ "$1" = --version ] && { %s; }\nexec %s "$@"\n' "$1" "$(command -v git)" >"$BATS_TEST_TMPDIR/oldgit/git"
	chmod +x "$BATS_TEST_TMPDIR/oldgit/git"
}

@test "doctor refuses a git too old for config hooks, naming the fix" {
	repo
	oldgit "echo 'git version 2.53.1'; exit 0"
	chmod 600 "$terms"
	PATH=$BATS_TEST_TMPDIR/oldgit:$PATH run "$AG" doctor
	[ "$status" -eq 1 ]
	has "$BATS_TEST_TMPDIR/oldgit/git (git version 2.53.1) skips config hooks"
	has "; fix: "
	lacks "no angerona-msg hook"
}

@test "doctor refuses a git that can't say its version" {
	repo
	oldgit "echo 'xcrun: error: invalid active developer path' >&2; exit 1"
	chmod 600 "$terms"
	PATH=$BATS_TEST_TMPDIR/oldgit:$PATH run "$AG" doctor
	[ "$status" -eq 1 ]
	has "(xcrun: error: invalid active developer path) skips config hooks"
}

@test "doctor's fix for an old git is a newer one later on PATH" {
	repo
	config_hooks
	oldgit "echo 'git version 2.53.1'; exit 0"
	chmod 600 "$terms"
	PATH=$BATS_TEST_TMPDIR/oldgit:$PATH run "$AG" doctor
	[ "$status" -eq 1 ]
	has "fix: put $(dirname "$(command -v git)") before $BATS_TEST_TMPDIR/oldgit on PATH"
}

@test "doctor reads a git's version past a warning before it" {
	repo
	hooks
	oldgit "echo 'git: error: could not create cache file' >&2; echo 'git version 2.56.0'; exit 0"
	chmod 600 "$terms"
	PATH=$BATS_TEST_TMPDIR/oldgit:$PATH run "$AG" doctor
	[ "$status" -eq 0 ]
	lacks "$BATS_TEST_TMPDIR/oldgit/git ("
}

@test "doctor notes an old git later on PATH, and still passes" {
	repo
	hooks
	oldgit "echo 'git version 2.39.5 (Apple Git-154)'; exit 0"
	chmod 600 "$terms"
	PATH=$PATH:$BATS_TEST_TMPDIR/oldgit run "$AG" doctor
	[ "$status" -eq 0 ]
	has "$BATS_TEST_TMPDIR/oldgit/git (git version 2.39.5 (Apple Git-154)) skips config hooks"
	has "fix: point any app that uses it at $(command -v git)"
	has "the guard is in place"
}

@test "doctor says which config switches a hook off" {
	repo
	hooks
	printf '[hook "angerona-staged"]\n\tenabled = false\n' >"$BATS_TEST_TMPDIR/work.gitconfig"
	git config --global "includeIf.gitdir:$BATS_TEST_TMPDIR/clone/.path" "$BATS_TEST_TMPDIR/work.gitconfig"
	chmod 600 "$terms"
	run "$AG" doctor
	[ "$status" -eq 1 ]
	has "angerona-staged is switched off here, by $BATS_TEST_TMPDIR/work.gitconfig"
	lacks "hook.angerona-staged.command"
	lacks "angerona-msg"
}

@test "doctor flags a hook switched off by the repo's own config" {
	repo
	hooks
	git config hook.angerona-push.enabled false
	chmod 600 "$terms"
	run "$AG" doctor
	[ "$status" -eq 1 ]
	has "angerona-push is switched off by this repo's own config, .git/config, which an agent can write"
	lacks "check-json"
	has "fix: git config unset --local hook.angerona-push.enabled"
	lacks "angerona-msg"
}

@test "doctor can't tell whether a hook is switched off when a hook key has no value" {
	repo
	hooks
	printf '[hook "angerona-push"]\n\tenabled\n' >>.git/config
	chmod 600 "$terms"
	run "$AG" doctor
	[ "$status" -eq 1 ]
	has "angerona-push: can't tell whether it is switched off here, as .git/config has hook.angerona-push.enabled with no value, which git can't read; fix: give it one"
	lacks "no angerona-push hook"
}

@test "doctor says the push hook is switched off exactly where check-json starts a work session" {
	repo
	hooks
	chmod 600 "$terms"
	agree() {
		local off=0 passes=0
		run "$AG" doctor
		[[ $output == *"angerona-push is switched off"* ]] && off=1
		CLAUDE_PROJECT_DIR=$BATS_TEST_TMPDIR/clone run "$AG" check-json <<<'{"tool_input":{"body":"for globex"}}'
		[ "$status" -eq 0 ] && passes=1
		[ "$off" -eq "$passes" ] || { echo "$1: doctor says off: $off, check-json passes: $passes"; return 1; }
	}
	agree "switched on"
	printf '[hook "angerona-push"]\n\tenabled = false\n' >"$BATS_TEST_TMPDIR/work.gitconfig"
	git config --global "includeIf.gitdir:$BATS_TEST_TMPDIR/clone/.path" "$BATS_TEST_TMPDIR/work.gitconfig"
	agree "a global include"
	GIT_CEILING_DIRECTORIES=$BATS_TEST_TMPDIR agree "ceiling directories"
	git config --global --unset "includeIf.gitdir:$BATS_TEST_TMPDIR/clone/.path"
	git config hook.angerona-push.enabled false
	agree "the repo's own config"
	git config --unset hook.angerona-push.enabled
	git config hook.pre-push.enabled false
	agree "every pre-push hook"
}

@test "a \$PWD that isn't the cwd doesn't pick the repo" {
	repo
	git init -q inner
	echo "for globex" >bad
	git add bad
	run env PWD="$BATS_TEST_TMPDIR/clone/inner" "$AG" check-staged
	[ "$status" -eq 1 ]
	has "bad:1: globex"
}

@test "a path through a symlink counts where git counts it: at a worktree's top, not below" {
	repo
	hooks
	chmod 600 "$terms"
	mkdir sub
	ln -s "$BATS_TEST_TMPDIR/clone" "$BATS_TEST_TMPDIR/link"
	printf '[hook "angerona-push"]\n\tenabled = false\n' >"$BATS_TEST_TMPDIR/work.gitconfig"
	git config --global "includeIf.gitdir:$BATS_TEST_TMPDIR/link/.path" "$BATS_TEST_TMPDIR/work.gitconfig"
	input='{"tool_input":{"body":"for globex"}}'
	cd "$BATS_TEST_TMPDIR/link"
	run git hook list pre-push
	has "disabled"
	run "$AG" doctor
	has "angerona-push is switched off here"
	CLAUDE_PROJECT_DIR=$BATS_TEST_TMPDIR/link run "$AG" check-json <<<"$input"
	[ "$status" -eq 0 ]
	cd sub
	run git hook list pre-push
	lacks "disabled"
	run "$AG" doctor
	lacks "angerona-push is switched off"
	CLAUDE_PROJECT_DIR=$BATS_TEST_TMPDIR/link/sub run "$AG" check-json <<<"$input"
	[ "$status" -eq 2 ]
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
	for text in GLOBEX_API_KEY=1 my_globex_client; do
		run "$AG" check <<<"$text"
		[ "$status" -eq 1 ] || { echo "passed: $text"; return 1; }
	done
	run "$AG" check <<<$'one\nglobex_x'
	has "line 2: globex"
}

@test "a term ending in punctuation matches before another word" {
	printf 'Globex Corp.\n' >"$terms"
	run "$AG" check <<<"Globex Corp. said hi"
	[ "$status" -eq 1 ]
	has "line 1: Globex Corp"
}

@test "check-push scans a renamed file's new name" {
	repo
	git mv README globex.txt
	git commit -qm "rename"
	run "$AG" check-push origin < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 1 ]
	has "file globex.txt: globex"
}

@test "check-push scans the name of a second file with the same content" {
	repo
	echo same >a.txt
	echo same >globex-notes.txt
	git add .
	git commit -qm "two"
	run "$AG" check-push origin < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 1 ]
	has "file globex-notes.txt: globex"
}

@test "check-push scans a submodule entry's name" {
	repo
	git update-index --add --cacheinfo "160000,$(git rev-parse HEAD),globex"
	git commit -qm "gitlink"
	run "$AG" check-push origin < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 1 ]
	has "file globex: globex"
}

@test "check-push refuses a push from a repo with grafts" {
	repo
	commit a fine
	echo "$(git rev-parse HEAD)" >.git/info/grafts
	run "$AG" check-push origin < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 1 ]
	has "grafts"
	rm .git/info/grafts
	: >"$BATS_TEST_TMPDIR/grafts"
	GIT_GRAFT_FILE=$BATS_TEST_TMPDIR/grafts run "$AG" check-push origin < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 1 ]
}

@test "check-push from a shallow clone scans what the push sends" {
	repo
	commit a "for globex"
	commit b fine
	commit c fine
	git clone -q --depth 1 "file://$BATS_TEST_TMPDIR/clone" "$BATS_TEST_TMPDIR/shallow"
	cd "$BATS_TEST_TMPDIR/shallow"
	commit d "a globex note"
	run "$AG" check-push origin "$BATS_TEST_TMPDIR/nothing.git" < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 1 ]
	has "d:1: globex"
}

@test "check-push refuses when a pushed blob is missing" {
	repo
	commit a fine
	id=$(git rev-parse HEAD:a)
	rm -f ".git/objects/$(echo "$id" | cut -c1-2)/$(echo "$id" | cut -c3-)"
	run "$AG" check-push origin "$BATS_TEST_TMPDIR/nothing.git" < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 1 ]
	has "git failed reading the push"
	has "$id"
}

@test "check-push asks the remote what it holds, not stale local refs" {
	repo
	commit a "for globex"
	git push -q origin main
	git init -q --bare "$BATS_TEST_TMPDIR/new.git"
	git remote set-url origin "$BATS_TEST_TMPDIR/new.git"
	run "$AG" check-push origin "$BATS_TEST_TMPDIR/new.git" < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 1 ]
	has "a:1: globex"
}

@test "check-push leaves out what a URL already holds" {
	repo
	commit a "for globex"
	git push -q origin main
	git commit -q --allow-empty -m neutral
	run "$AG" check-push "$BATS_TEST_TMPDIR/origin.git" < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 0 ]
}

@test "check-push asks the URL it was given, without rewriting it again" {
	repo
	commit a "for globex"
	git push -q origin main
	git init -q --bare "$BATS_TEST_TMPDIR/other.git"
	git config --global "url.$BATS_TEST_TMPDIR/other.git.insteadOf" "$BATS_TEST_TMPDIR/origin.git"
	git commit -q --allow-empty -m neutral
	run "$AG" check-push origin "$BATS_TEST_TMPDIR/origin.git" < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 0 ]
}

@test "check-push scans the whole history when it can't ask the remote, and says so" {
	repo
	commit a "for globex"
	git push -q origin main
	git commit -q --allow-empty -m neutral
	run "$AG" check-push origin "$BATS_TEST_TMPDIR/nowhere.git" < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 1 ]
	has "can't ask $BATS_TEST_TMPDIR/nowhere.git what it holds"
}

@test "check-push keeps a URL's credentials out of its note" {
	repo
	commit a "for globex"
	git push -q origin main
	git commit -q --allow-empty -m neutral
	run "$AG" check-push origin "http://user:s3cret@127.0.0.1:9/r.git" < <(push_line HEAD refs/heads/main)
	[ "$status" -eq 1 ]
	has "can't ask http://"
	lacks "s3cret"
	# A token can sit in the user field.
	run "$AG" check-push origin "https://t0ken@127.0.0.1:9/r.git" < <(push_line HEAD refs/heads/main)
	has "can't ask https://"
	lacks "t0ken"
}

@test "check-json scans keys and input that is not an object" {
	run "$AG" check-json <<<'{"tool_input":{"globex":1}}'
	[ "$status" -eq 2 ]
	has "key globex: globex"
	run "$AG" check-json <<<'{"tool_input":"for globex"}'
	[ "$status" -eq 2 ]
	has "globex"
}

@test "check-msg skips the comments and diff git strips from a message" {
	printf 'remove the leaked file\n# Changes to be committed:\n#\tdeleted: globex.txt\n# ------------------------ >8 ------------------------\ndiff --git a/globex.txt b/globex.txt\n-for globex\n' >"$BATS_TEST_TMPDIR/msg"
	run "$AG" check-msg "$BATS_TEST_TMPDIR/msg"
	[ "$status" -eq 0 ]
}

@test "doctor refuses hooks whose command is gone" {
	repo
	hooks
	git config --global hook.angerona-push.command "/nonexistent/angerona check-push"
	chmod 600 "$terms"
	run "$AG" doctor
	[ "$status" -eq 1 ]
	has "no angerona-push hook for pre-push"
}

@test "doctor checks the Claude Code hook when ~/.claude exists" {
	repo
	hooks
	chmod 600 "$terms"
	mkdir -p "$HOME/.claude"
	echo '{"hooks":{"PreToolUse":[{"matcher":"mcp__(.*_)?[Gg]it[Hh]ub__","hooks":[{"type":"command","command":"/x/angerona","args":["check-json"]}]}]}}' >"$HOME/.claude/settings.json"
	run "$AG" doctor
	[ "$status" -eq 1 ]
	has "no Claude Code hook runs angerona check-json on every MCP call"
	echo '{"hooks":{"PreToolUse":[{"matcher":"mcp__.*","hooks":[{"type":"command","command":"/x/angerona","args":["check-json"],"onFailure":"block"}]}]}}' >"$HOME/.claude/settings.json"
	run "$AG" doctor
	[ "$status" -eq 0 ]
}

# ~/.claude/settings.json with a PreToolUse hook on MATCHER that runs
# check-json and blocks on failure, and EXTRA, a JSON member, beside it.
claude_hook() { # MATCHER [EXTRA]
	mkdir -p "$HOME/.claude"
	printf '{%s"hooks":{"PreToolUse":[{"matcher":"%s","hooks":[{"type":"command","command":"/x/angerona","args":["check-json"],"onFailure":"block"}]}]}}\n' "${2:+$2,}" "$1" >"$HOME/.claude/settings.json"
}

@test "doctor refuses a Claude Code matcher that is not every MCP call" {
	repo
	hooks
	chmod 600 "$terms"
	for m in mcp__ mcp mcp__x__ 'mcp__github__.*' 'Edit|Write' 'mcp__a, mcp__b'; do
		claude_hook "$m"
		run "$AG" doctor
		[ "$status" -eq 1 ] || { echo "passed: $m"; return 1; }
		has "no Claude Code hook runs angerona check-json on every MCP call"
	done
}

@test "doctor accepts a Claude Code matcher that takes every tool" {
	repo
	hooks
	chmod 600 "$terms"
	for m in '*' '' '.*' 'mcp__.*'; do
		claude_hook "$m"
		run "$AG" doctor
		[ "$status" -eq 0 ] || { echo "refused: $m"; return 1; }
	done
}

@test "doctor refuses a Claude Code hook that disableAllHooks switches off" {
	repo
	hooks
	chmod 600 "$terms"
	claude_hook 'mcp__.*' '"disableAllHooks":false'
	run "$AG" doctor
	[ "$status" -eq 0 ]
	claude_hook 'mcp__.*' '"disableAllHooks":true'
	run "$AG" doctor
	[ "$status" -eq 1 ]
	has "no Claude Code hook runs angerona check-json on every MCP call"
}

@test "doctor accepts the Claude Code hook in README.md, but only with onFailure block" {
	repo
	hooks
	chmod 600 "$terms"
	mkdir -p "$HOME/.claude"
	echo '{"hooks":{"PreToolUse":[{"matcher":"mcp__.*","hooks":[{"type":"command","command":"/Users/you/.local/bin/angerona","args":["check-json"],"onFailure":"block"}]}]}}' >"$HOME/.claude/settings.json"
	run "$AG" doctor
	[ "$status" -eq 0 ]
	# The shell form works too.
	echo '{"hooks":{"PreToolUse":[{"matcher":"mcp__.*","hooks":[{"type":"command","command":"~/.local/bin/angerona check-json || exit 2","onFailure":"block"}]}]}}' >"$HOME/.claude/settings.json"
	run "$AG" doctor
	[ "$status" -eq 0 ]
	echo '{"hooks":{"PreToolUse":[{"matcher":"mcp__.*","hooks":[{"type":"command","command":"/Users/you/.local/bin/angerona","args":["check-json"]}]}]}}' >"$HOME/.claude/settings.json"
	run "$AG" doctor
	[ "$status" -eq 1 ]
	has "no Claude Code hook runs angerona check-json on every MCP call"
}
