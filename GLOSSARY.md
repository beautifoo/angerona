# angerona

Keeps the names on a private list out of what you publish with git and what an agent sends to an MCP server.

## Language

**Term**:
A name on the list that must stay out of what you publish, matched as written, in any case, even inside a longer word.
_Avoid_: word, pattern, secret

**List**:
The private file of terms, kept outside every repo.
_Avoid_: term file, blocklist

**Guard**:
The three git hooks and the Claude Code hook that refuse anything naming a term.
_Avoid_: filter, scanner

**Switched off**:
A guard hook is switched off in a repo when git's config there tells git not to run it, on purpose where the terms belong.
_Avoid_: disabled, turned off, bypassed

**Work session**:
A Claude Code session whose project is a repo where the push hook is switched off; its MCP calls pass unscanned, except a GitHub call that names no owner, or one that isn't the repo's own.
_Avoid_: work mode, work repo
