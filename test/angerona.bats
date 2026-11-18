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
	commit x/jdoe.xcuserdatad/y hello
	run git push origin main
	[ "$status" -eq 1 ]
	has jdoe.xcuserdatad
}

@test "my email as author blocks" {
	repo
	printf 'hello\n' >a.txt
	git add a.txt
	git -c user.email=me@home.example commit -qm a
	run git push origin main
	[ "$status" -eq 1 ]
	has me@home.example
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
