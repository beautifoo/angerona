#!/usr/bin/env bats
# angerona against throwaway repos. Run: bats test/

setup() {
	AG=${AG:-$BATS_TEST_DIRNAME/../target/debug/angerona}
	export XDG_CONFIG_HOME=$BATS_TEST_TMPDIR/config HOME=$BATS_TEST_TMPDIR/home
	mkdir -p "$HOME"
	# Keep the user's git config (signing, aliases, colours) out of the tests.
	export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=$BATS_TEST_TMPDIR/gitconfig
	unset EMAIL GIT_AUTHOR_EMAIL GIT_COMMITTER_EMAIL
	git config --global user.name Tester
	git config --global user.email 1+tester@users.noreply.github.com
	git config --global init.defaultBranch main
	# An email set only under a condition that never holds is still yours.
	git config --global includeIf.gitdir:/nonexistent/.path "$BATS_TEST_TMPDIR/work.gitconfig"
	git config --file "$BATS_TEST_TMPDIR/work.gitconfig" user.email me@home.example
	export USER=jdoe ANGERONA_TEST_HOSTS=jdoes-laptop ANGERONA_TEST_VISIBILITY=public
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

# A repo r with the hook, pushing to the bare origin.git.
repo() {
	git init -q --bare "$BATS_TEST_TMPDIR/origin.git"
	git init -q "$BATS_TEST_TMPDIR/r"
	cd "$BATS_TEST_TMPDIR/r"
	git remote add origin "$BATS_TEST_TMPDIR/origin.git"
	git config hook.angerona.event pre-push
	git config hook.angerona.command "$AG check-push"
}

# Writes the file with the text, then commits it.
commit() {
	mkdir -p "$(dirname "$1")"
	printf '%s\n' "$2" >"$1"
	git add "$1"
	git commit -qm "add $1"
}

# Pushes as the private remote, so nothing is checked.
push_private() {
	ANGERONA_TEST_VISIBILITY=private git push -q origin "$@"
}

@test "usage on an unknown command" {
	run "$AG" nonsense
	[ "$status" -eq 2 ]
	has "usage:"
}

@test "a clean push passes" {
	repo
	commit a.txt hello
	run git push origin main
	[ "$status" -eq 0 ]
}

@test "a home path in a new file blocks" {
	repo
	commit a.txt /Users/jdoe/x
	run git push origin main
	[ "$status" -eq 1 ]
	has /Users/jdoe
}

@test "an old home path in an edited file passes" {
	repo
	commit a.txt /Users/jdoe/x
	push_private main
	printf 'more\n' >>a.txt
	git commit -qam more
	run git push origin main
	[ "$status" -eq 0 ]
}

@test "a line added then removed in the same push still blocks" {
	repo
	commit a.txt /Users/jdoe/x
	commit a.txt clean
	run git push origin main
	[ "$status" -eq 1 ]
	has /Users/jdoe
}

@test "a merge conflict edit is checked" {
	repo
	commit a.txt one
	git switch -q -c side
	commit a.txt side
	git switch -q main
	commit a.txt main
	git merge -q side || true
	printf '/Users/jdoe/x\n' >a.txt
	git commit -qam merge
	push_private side
	run git push origin main
	[ "$status" -eq 1 ]
	has "a.txt:1"
}

@test "a merged-in side is not re-reported" {
	repo
	commit a.txt one
	push_private main
	git switch -q -c side
	commit b.txt /Users/jdoe/x
	push_private side
	git switch -q main
	commit c.txt two
	git merge -q --no-edit side
	run git push origin main
	[ "$status" -eq 0 ]
}

@test "a pure rename passes" {
	repo
	commit a.txt /Users/jdoe/x
	push_private main
	git mv a.txt b.txt
	git commit -qm rename
	run git push origin main
	[ "$status" -eq 0 ]
}

@test "a renamed and edited file reports only the new line" {
	repo
	printf 'one\ntwo\nthree\n/Users/jdoe/old\n' >a.txt
	git add a.txt
	git commit -qm a
	push_private main
	git mv a.txt b.txt
	printf '/Users/jdoe/new\n' >>b.txt
	git commit -qam rename
	run git push origin main
	[ "$status" -eq 1 ]
	has "b.txt:5"
	lacks "b.txt:4"
}

@test "a binary file with a home path blocks" {
	repo
	printf 'x\0\1\2 path=/Users/jdoe/projects/x \0' >bin.dat
	git add bin.dat
	git commit -qm bin
	run git push origin main
	[ "$status" -eq 1 ]
	has /Users/jdoe
}

@test "a UTF-16 file with a home path blocks" {
	repo
	{ printf '\377\376'; printf '/Users/jdoe/x\n' | iconv -f UTF-8 -t UTF-16LE; } >u.txt
	git add u.txt
	git commit -qm utf16
	run git push origin main
	[ "$status" -eq 1 ]
	has /Users/jdoe
}

@test "a file path with the login blocks" {
	repo
	mkdir -p x/jdoe.xcuserdatad
	printf 'hello\n' >x/jdoe.xcuserdatad/y
	git add x
	git commit -qm hello
	run git push origin main
	[ "$status" -eq 1 ]
	has "file x/jdoe.xcuserdatad/y"
}

@test "my email as author blocks" {
	repo
	printf 'hello\n' >a.txt
	git add a.txt
	git -c user.email=me@home.example commit -qm a
	run git push origin main
	[ "$status" -eq 1 ]
	has "author  email me@home.example"
}

@test "my email in a Co-authored-by trailer blocks" {
	repo
	printf 'hello\n' >a.txt
	git add a.txt
	git commit -qm a -m "Co-authored-by: Me <me@home.example>"
	run git push origin main
	[ "$status" -eq 1 ]
	has me@home.example
}

@test "an unknown committer email on a new commit blocks" {
	repo
	printf 'hello\n' >a.txt
	git add a.txt
	GIT_COMMITTER_EMAIL=bob@corp.example git commit -qm a
	run git push origin main
	[ "$status" -eq 1 ]
	has bob@corp.example
}

@test "an unknown author email on an applied patch passes" {
	repo
	printf 'hello\n' >a.txt
	git add a.txt
	git commit -qm a --author "Bob <bob@corp.example>"
	run git push origin main
	[ "$status" -eq 0 ]
}

@test "an upstream contributor's email on a fork sync passes" {
	repo
	commit a.txt one
	push_private main
	git init -q --bare "$BATS_TEST_TMPDIR/upstream.git"
	git clone -q "$BATS_TEST_TMPDIR/origin.git" "$BATS_TEST_TMPDIR/up"
	(
		cd "$BATS_TEST_TMPDIR/up"
		printf 'two\n' >>a.txt
		git -c user.email=bob@corp.example commit -qam two
		git push -q "$BATS_TEST_TMPDIR/upstream.git" main
	)
	git remote add upstream "$BATS_TEST_TMPDIR/upstream.git"
	git fetch -q upstream
	git merge -q --ff-only upstream/main
	run git push origin main
	[ "$status" -eq 0 ]
}

@test "a tag on a pushed commit is checked" {
	repo
	commit a.txt hello
	git push -q origin main
	GIT_COMMITTER_EMAIL=bob@corp.example git tag -a v1 -m v1
	run git push origin v1
	[ "$status" -eq 1 ]
	has bob@corp.example
}

@test "a tag on a tree is checked" {
	repo
	commit a.txt hello
	commit d/b.txt /Users/jdoe/x
	git tag -a t -m t 'HEAD^{tree}'
	run git push origin t
	[ "$status" -eq 1 ]
	has "d/b.txt:1"
}

@test "a branch named after my login blocks" {
	repo
	git switch -q -c jdoe/fix
	commit a.txt hello
	run git push origin jdoe/fix
	[ "$status" -eq 1 ]
	has refs/heads/jdoe/fix
}

@test "the local branch name is not checked" {
	repo
	git switch -q -c jdoe/fix
	commit a.txt hello
	run git push origin jdoe/fix:refs/heads/fix
	[ "$status" -eq 0 ]
}

@test "a private remote is not checked" {
	repo
	commit a.txt /Users/jdoe/x
	ANGERONA_TEST_VISIBILITY=private run git push origin main
	[ "$status" -eq 0 ]
}

@test "a deletion-only push passes" {
	repo
	commit a.txt /Users/jdoe/x
	push_private main main:other
	run git push origin :other
	[ "$status" -eq 0 ]
}

@test "a Latin-1 message still matches" {
	repo
	printf 'hello\n' >a.txt
	git add a.txt
	printf 'caf\351 /Users/jdoe\n' >"$BATS_TEST_TMPDIR/msg"
	git commit -q -F "$BATS_TEST_TMPDIR/msg"
	run git push origin main
	[ "$status" -eq 1 ]
	has /Users/jdoe
}

@test "the hook works from a linked worktree" {
	repo
	commit a.txt hello
	git worktree add -q "$BATS_TEST_TMPDIR/wt" -b wt
	cd "$BATS_TEST_TMPDIR/wt"
	commit b.txt /Users/jdoe/x
	run git push origin wt
	[ "$status" -eq 1 ]
	has /Users/jdoe
}

@test "an unreadable object refuses the push" {
	repo
	commit a.txt hello
	blob=$(git rev-parse HEAD:a.txt)
	object=.git/objects/${blob:0:2}/${blob:2}
	chmod u+w "$object"
	printf 'garbage' >"$object"
	run git push origin main
	[ "$status" -eq 1 ]
	has "refusing to push"
}

# Runs the command with a terminal that answers with INPUT, as the hook's /dev/tty.
with_tty() {
	local input=$1
	shift
	export TERM=xterm
	if [[ $(uname) == Darwin ]]; then
		{ printf '%s' "$input"; sleep 1; } | script -q /dev/null "$@"
	else
		{ printf '%s' "$input"; sleep 1; } | script -qec "$(printf '%q ' "$@")" /dev/null
	fi
}

@test "answering y pushes past a finding" {
	repo
	commit a.txt /Users/jdoe/x
	run with_tty $'y\n' git push origin main
	[ "$status" -eq 0 ]
	has "Push anyway?"
}

@test "the default is no" {
	repo
	commit a.txt /Users/jdoe/x
	run with_tty $'\n' git push origin main
	[ "$status" -eq 1 ]
	has "other hooks may still refuse this push"
	has "Push anyway?"
	lacks "--no-verify"
}

@test "without a terminal a finding blocks and names --no-verify" {
	repo
	commit a.txt /Users/jdoe/x
	TERM=dumb run git push origin main
	[ "$status" -eq 1 ]
	has "--no-verify"
}

@test "an unprobeable URL asks on every push and saves nothing" {
	repo
	commit a.txt /Users/jdoe/x
	export ANGERONA_TEST_VISIBILITY=unprobeable
	run with_tty $'n\n' git push origin main
	[ "$status" -eq 0 ]
	commit b.txt /Users/jdoe/y
	run with_tty $'n\n' git push origin main
	[ "$status" -eq 0 ]
	has "Treat"
	run git config --local --get-regexp '^angerona\.'
	[ "$status" -eq 1 ]
}

@test "an unprobeable URL with no terminal is checked as public" {
	repo
	export ANGERONA_TEST_VISIBILITY=unprobeable
	commit a.txt me@home.example
	TERM=dumb run git push origin main
	[ "$status" -eq 1 ]
	has "email me@home.example"
	lacks "Treat"
}

@test "a passing failure checks without asking" {
	repo
	commit a.txt /Users/jdoe/x
	export ANGERONA_TEST_VISIBILITY=passing
	run with_tty $'n\n' git push origin main
	[ "$status" -eq 1 ]
	lacks "Treat"
}

# A bin dir holding only git and a fake gh that runs the given script body.
fake_gh() {
	mkdir -p "$BATS_TEST_TMPDIR/bin"
	ln -sf "$(command -v git)" "$BATS_TEST_TMPDIR/bin/git"
	printf '#!/bin/sh\n%s\n' "$1" >"$BATS_TEST_TMPDIR/bin/gh"
	chmod +x "$BATS_TEST_TMPDIR/bin/gh"
}

# A bin dir holding only git: no gh on PATH.
no_gh() {
	mkdir -p "$BATS_TEST_TMPDIR/bin"
	ln -sf "$(command -v git)" "$BATS_TEST_TMPDIR/bin/git"
}

# A repo r pushing to origin.git, with only the global hook from setup.
global_repo() {
	git init -q --bare "$BATS_TEST_TMPDIR/origin.git"
	git init -q "$BATS_TEST_TMPDIR/r"
	cd "$BATS_TEST_TMPDIR/r"
	git remote add origin "$BATS_TEST_TMPDIR/origin.git"
}

HOOK='f() { if command -v angerona >/dev/null 2>&1; then angerona check-push "$@"; else echo '\''angerona not installed, push not checked'\'' >&2; fi; }; f'

@test "setup writes the hook into the global config" {
	no_gh
	PATH=$BATS_TEST_TMPDIR/bin run "$AG" setup
	[ "$status" -eq 0 ]
	[ "$(git config --global hook.angerona.command)" = "$HOOK" ]
}

@test "setup sets the pre-push event" {
	no_gh
	PATH=$BATS_TEST_TMPDIR/bin run "$AG" setup
	[ "$(git config --global hook.angerona.event)" = pre-push ]
}

@test "setup keeps the home path out of the global config" {
	no_gh
	PATH=$BATS_TEST_TMPDIR/bin run "$AG" setup
	run grep -e /Users/ -e '$HOME' "$GIT_CONFIG_GLOBAL"
	[ "$status" -eq 1 ]
}

@test "setup writes through a symlinked config" {
	no_gh
	printf '' >"$BATS_TEST_TMPDIR/real"
	ln -s "$BATS_TEST_TMPDIR/real" "$BATS_TEST_TMPDIR/link"
	export GIT_CONFIG_GLOBAL=$BATS_TEST_TMPDIR/link
	PATH=$BATS_TEST_TMPDIR/bin run "$AG" setup
	[ -L "$BATS_TEST_TMPDIR/link" ]
	grep -q 'event = pre-push' "$BATS_TEST_TMPDIR/real"
}

@test "the hook line runs angerona from PATH" {
	no_gh
	PATH=$BATS_TEST_TMPDIR/bin run "$AG" setup
	mkdir "$BATS_TEST_TMPDIR/ag"
	ln -s "$AG" "$BATS_TEST_TMPDIR/ag/angerona"
	global_repo
	commit a.txt /Users/jdoe/x
	TERM=dumb PATH=$BATS_TEST_TMPDIR/ag:$BATS_TEST_TMPDIR/bin:/bin run git push origin main
	[ "$status" -eq 1 ]
	has "this push names you"
}

@test "without angerona the push passes with a warning" {
	no_gh
	PATH=$BATS_TEST_TMPDIR/bin run "$AG" setup
	global_repo
	commit a.txt /Users/jdoe/x
	PATH=$BATS_TEST_TMPDIR/bin:/bin run git push origin main
	[ "$status" -eq 0 ]
	has "angerona not installed, push not checked"
}

@test "setup saves my GitHub emails privately" {
	fake_gh 'printf "%s\n" Me@Personal.Example 1+me@users.noreply.github.com'
	PATH=$BATS_TEST_TMPDIR/bin run "$AG" setup
	[ "$(cat "$XDG_CONFIG_HOME/angerona/emails")" = me@personal.example ]
}

@test "the saved emails file is private" {
	fake_gh 'printf "%s\n" Me@Personal.Example'
	PATH=$BATS_TEST_TMPDIR/bin run "$AG" setup
	f=$XDG_CONFIG_HOME/angerona/emails
	[ -n "$(find "$f" -perm 600)" ]
}

@test "a saved email in a pushed file is refused" {
	fake_gh 'printf "%s\n" Me@Personal.Example'
	PATH=$BATS_TEST_TMPDIR/bin run "$AG" setup
	repo
	commit a.txt me@personal.example
	TERM=dumb run git push origin main
	[ "$status" -eq 1 ]
}

@test "setup asks for the user:email scope" {
	fake_gh 'echo "This API operation needs the \"user\" scope" >&2; exit 1'
	PATH=$BATS_TEST_TMPDIR/bin run "$AG" setup
	[ "$status" -eq 0 ]
	has "run: gh auth refresh -h github.com -s user:email, then angerona setup again"
}

@test "setup without gh still writes the hook" {
	no_gh
	PATH=$BATS_TEST_TMPDIR/bin run "$AG" setup
	[ "$status" -eq 0 ]
	has "gh not available"
}

@test "scan of a clean repo passes" {
	repo
	commit a.txt hello
	run "$AG" scan
	[ "$status" -eq 0 ]
}

@test "scan reports an old home path deleted since" {
	repo
	commit a.txt /Users/jdoe/x
	git rm -q a.txt
	git commit -qm remove
	run "$AG" scan
	[ "$status" -eq 1 ]
	has /Users/jdoe
}

@test "scan reports a current file" {
	repo
	commit a.txt /Users/jdoe/x
	run "$AG" scan
	has "current a.txt:1"
}

@test "scan reports any committer email" {
	repo
	commit a.txt hello
	GIT_COMMITTER_EMAIL=bob@corp.example git commit -q --amend --no-edit --allow-empty
	run "$AG" scan
	has bob@corp.example
}

@test "scan warns on a shallow clone" {
	repo
	commit a.txt one
	commit b.txt two
	git clone -q --depth 1 "file://$PWD" "$BATS_TEST_TMPDIR/s"
	cd "$BATS_TEST_TMPDIR/s"
	run "$AG" scan
	has "this clone is shallow or partial"
}

@test "scan never changes the repo" {
	repo
	commit a.txt /Users/jdoe/x
	before=$(git rev-parse --all; git status --porcelain)
	run "$AG" scan
	[ "$(git rev-parse --all; git status --porcelain)" = "$before" ]
}

@test "scan reads history only a detached HEAD reaches" {
	repo
	commit a.txt /Users/jdoe/x
	git rm -q a.txt
	git commit -qm remove
	git checkout -q --detach
	git branch -D main -q
	run "$AG" scan
	[ "$status" -eq 1 ]
	has /Users/jdoe
}

@test "scan reports any tagger email" {
	repo
	commit a.txt hello
	GIT_COMMITTER_EMAIL=bob@corp.example git tag -a v1 -m release
	run "$AG" scan
	[ "$status" -eq 1 ]
	has bob@corp.example
}

# A setup that doctor accepts: the hook and emails file from setup, and
# useConfigOnly. Needs fake_gh first.
good_setup() {
	git config --global user.useConfigOnly true
	PATH=$BATS_TEST_TMPDIR/bin "$AG" setup >/dev/null
}

@test "doctor passes a good setup" {
	fake_gh 'printf "%s\n" Me@Personal.Example'
	good_setup
	PATH=$BATS_TEST_TMPDIR/bin run "$AG" doctor
	[ "$status" -eq 0 ]
	lacks "problem:"
}

@test "doctor flags a stale emails file" {
	fake_gh 'printf "%s\n" Me@Personal.Example'
	good_setup
	fake_gh 'printf "%s\n" Me@Personal.Example new@Other.Example'
	PATH=$BATS_TEST_TMPDIR/bin run "$AG" doctor
	[ "$status" -eq 1 ]
	has "fix: angerona setup"
}

@test "doctor flags a missing useConfigOnly" {
	fake_gh 'printf "%s\n" Me@Personal.Example'
	good_setup
	git config --global --unset user.useConfigOnly
	PATH=$BATS_TEST_TMPDIR/bin run "$AG" doctor
	[ "$status" -eq 1 ]
	has "fix: git config set --global user.useConfigOnly true"
}

@test "doctor flags a real global email" {
	fake_gh 'printf "%s\n" Me@Personal.Example'
	good_setup
	git config --global user.email me@real.example
	PATH=$BATS_TEST_TMPDIR/bin run "$AG" doctor
	[ "$status" -eq 1 ]
	has "+<user>@users.noreply.github.com"
}

@test "doctor flags a signing key set as an email" {
	fake_gh 'printf "%s\n" Me@Personal.Example'
	good_setup
	git config --global user.signingkey me@real.example
	PATH=$BATS_TEST_TMPDIR/bin run "$AG" doctor
	[ "$status" -eq 1 ]
	has "user.signingkey"
}

@test "doctor flags a missing hook" {
	fake_gh 'printf "%s\n" Me@Personal.Example'
	good_setup
	git config --global --unset hook.angerona.command
	PATH=$BATS_TEST_TMPDIR/bin run "$AG" doctor
	[ "$status" -eq 1 ]
	has "hook.angerona.command"
}

@test "doctor notes a weak login" {
	fake_gh 'printf "%s\n" Me@Personal.Example'
	good_setup
	USER=dev PATH=$BATS_TEST_TMPDIR/bin run "$AG" doctor
	has "note: login dev is too common to match"
}

@test "doctor with no identity says how to set one" {
	fake_gh 'printf "%s\n" Me@Personal.Example'
	good_setup
	git config --global --unset user.email
	mkdir -p "$BATS_TEST_TMPDIR/empty"
	HOME=$BATS_TEST_TMPDIR/empty PATH=$BATS_TEST_TMPDIR/bin run "$AG" doctor
	has "no global user.email"
}

@test "doctor ends with the scan reminder" {
	fake_gh 'printf "%s\n" Me@Personal.Example'
	good_setup
	PATH=$BATS_TEST_TMPDIR/bin run "$AG" doctor
	[ "${lines[${#lines[@]}-1]}" = "angerona: repos already public are not rechecked; run angerona scan in each" ]
}
