---
name: reviewing-prs
description: "Agent-powered GitHub PR reviews with smart semantic triage. Categorizes changes as MECHANICAL (skip), NEW LOGIC (read), or BEHAVIORAL (verify) — so agents never waste tokens reading lock files or formatting diffs. Includes remote file reading, text/AST search across PR or full repo, and comment posting. No local clone needed. Use when asked to review a PR, check a pull request, look at PR changes, or given a PR number/URL to review."
---

# PR Review with gh-agent

Works from any directory — everything is fetched from GitHub API.

## Install gh-agent

```bash
brew install ataraxy-labs/tap/gh-agent
```

Requires `GITHUB_TOKEN` env var or [GitHub CLI](https://cli.github.com/) authenticated via `gh auth login`.

## Workflow

**1. Triage** → `gh-agent pr view --repo OWNER/REPO N --smart`

Categorizes changes into MECHANICAL (skip), NEW LOGIC (read), BEHAVIORAL (verify inline).

**2. Diffs** → `gh-agent pr diff --repo OWNER/REPO N --smart-files`

Gets diffs for non-mechanical files only. Lock/generated/minified files excluded by default.

**3. Impact analysis** — after reading diffs, search for breakage outside the PR:

Identify removed/renamed exports, changed signatures, deleted types, or modified public APIs from the diff. Then search `--repo-wide` for each:

```bash
# Find callers of a removed/renamed symbol
gh-agent pr grep --repo OWNER/REPO N --pattern "removedFunction" --repo-wide

# Find references to a deleted type or enum variant
gh-agent pr grep --repo OWNER/REPO N --pattern "DeletedTypeName" --repo-wide

# Find consumers of a changed interface/config
gh-agent pr grep --repo OWNER/REPO N --pattern "changedOption" --repo-wide --path src/

# Structural search for removed prop usage
gh-agent pr ast-grep --repo OWNER/REPO N --pattern 'oldPropName={$$$}' --repo-wide
```

Search for:
- **Deleted exports/types** — still imported elsewhere?
- **Removed function parameters** — callers updated?
- **Changed constants/config** — validators, serializers, tests in sync?
- **Removed attributes/flags** — renderers, parsers, importers still reference them?

**4. Context** — when diffs or impact results need more detail:

```bash
# Read full file at PR branch
gh-agent pr file --repo OWNER/REPO N --path PATH

# Search PR changed files (fast, default)
gh-agent pr grep --repo OWNER/REPO N --pattern "functionName"
gh-agent pr ast-grep --repo OWNER/REPO N --pattern 'useCallback($$$)'
```

`--repo-wide` uses GitHub Code Search + always includes PR files at head ref. PR results win on overlap. `--base` searches the base branch instead.

**5. Review** — you are an expert senior engineer with deep knowledge of software engineering best practices, security, performance, and maintainability. Perform a thorough code review of the collected diffs and impact results:

1. Generate a high-level summary of the changes in the diff.
2. Go file-by-file and review each changed hunk.
3. Comment on what changed in that hunk (including the line range) and how it relates to other changed hunks and code, reading any other relevant files. Also call out bugs, hackiness, unnecessary code, or too much shared mutable state.
4. Flag any `--repo-wide` hits from impact analysis that indicate broken callers, stale references, or missing updates outside the PR.
5. Categorize findings by severity: CRITICAL, HIGH, MEDIUM, LOW.

**6. Post** (only when user asks):

```bash
# Post comments (line must appear in diff — use --json to check)
gh-agent pr review --repo OWNER/REPO N --comments-file /tmp/review.json
gh-agent pr diff --repo OWNER/REPO N --json   # commentable lines map

# Post suggestion
gh-agent pr suggest --repo OWNER/REPO N --file F --line-start S --line-end E --replacement "code"
```

## Commands

| Command | Purpose |
|---|---|
| `pr view --repo R N --smart` | Smart triage — always start here |
| `pr view --repo R N --json` | PR metadata as JSON |
| `pr diff --repo R N --smart-files` | Diffs for non-mechanical files only |
| `pr diff --repo R N --file F` | Diff for specific file(s) (substring match, repeatable) |
| `pr diff --repo R N --stat` | File stat table |
| `pr diff --repo R N --json` | Commentable lines map |
| `pr file --repo R N --path P` | Read file at PR branch |
| `pr grep --repo R N -p PAT` | Text search PR changed files |
| `pr grep --repo R N -p PAT --repo-wide` | Text search full codebase |
| `pr ast-grep --repo R N -p PAT` | Structural search PR changed files |
| `pr ast-grep --repo R N -p PAT --repo-wide` | Structural search full codebase |
| `pr review --repo R N -c F` | Post review from JSON |
| `pr suggest --repo R N ...` | Post suggestion comment |

## Rules

1. **`--smart` first**. Never read all diffs blindly.
2. **`--smart-files` for diffs**. One call, no manual file list.
3. **Skip MECHANICAL**. Don't read or comment on them.
4. **NEVER use local files**. Always use `pr file`, `pr grep`, or `pr ast-grep`.
5. **Always do impact analysis**. After reading diffs, `--repo-wide` grep for every deleted/renamed export, removed type, and changed public API. This catches broken callers outside the PR.
6. **Search incrementally**. PR files first → `--repo-wide` only if needed for additional context.
7. **Review in-skill**. Do not hand off to external review tools. Summarize, go hunk-by-hunk, flag bugs and stale references, and categorize by severity.
8. **Post only when asked**. Present findings in chat; user decides when to post.
