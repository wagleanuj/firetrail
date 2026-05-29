# Project Instructions for AI Agents

This file provides instructions and context for AI coding agents working on this project.

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:ca08a54f -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

### Filing children under an epic

When you file an issue that belongs under an existing epic, use the `--parent` flag —
do NOT use `bd dep add child epic` with the default `blocks` type. The default
inverts the relationship (epic blocks child), which forces `--force` closes later.

```bash
# Correct: epic depends on child (parent-child); epic auto-unblocks when children close
bd create --parent firetrail-44v --title "Reopen route" --type=feature --priority=1

# If the issue already exists:
bd dep add firetrail-44v firetrail-n77 --type=parent-child
```

### Closing issues via commit messages

A `post-commit` hook at `.beads/hooks/post-commit` parses commit messages and
auto-closes any issues referenced with trailer-style keywords:

```
Closes: firetrail-abc
Fixes: firetrail-abc, firetrail-xyz
Resolves: firetrail-abc
```

The colon is required; bare prose like "this closes the dialog" is ignored.
Set `BD_NO_AUTOCLOSE=1` to opt out for a single commit.

This means once a commit lands you do NOT need to also `bd close` — the hook
handles it. Still run `bd close --reason ...` manually for issues that
ship without a code change (e.g. won't-fix, deferred to ADR).

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd dolt push
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds
<!-- END BEADS INTEGRATION -->


## Build & Test

_Add your build and test commands here_

```bash
# Example:
# npm install
# npm test
```

## Architecture Overview

_Add a brief overview of your project architecture_

## Conventions & Patterns

_Add your project-specific conventions here_
