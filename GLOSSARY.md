# angerona

Keeps you out of what you publish with git: your emails, your home path, your login name and your machine's name.

## Language

**Personal detail**:
One of your emails, your home path, your login name or your machine's name, as found on the machine; your full name is not one.
_Avoid_: trace, identity, PII, term

**Outgoing commit**:
A commit a push sends that the repo it goes to doesn't have yet.
_Avoid_: pushed commit, new commit

**Unpublished commit**:
An outgoing commit that no remote you know of has yet, so nobody else can have published it.
_Avoid_: new commit, local commit

**Public repo**:
A repo anyone can read without logging in, or one you told angerona to treat as public.
_Avoid_: open repo, published repo

**Noreply address**:
An email a git host gives you to sign commits with in place of your real one, safe to publish.
_Avoid_: alias, private email

**Repo's own email**:
The email git uses in a repo, when it is shaped like an address and differs from the one git uses outside any repo; never reported in that repo.
_Avoid_: allowed address, skipped email, ignored email, exempt address
