---
name: reviewing-prs
description: "Collects PR context using gh-agent CLI with smart semantic triage. Use when asked to review a PR, check a pull request, look at PR changes, or given a PR number/URL to review. Pairs with code-review skill for the actual review."
---

# PR Review with gh-agent

Works from any directory — everything is fetched from GitHub API.

## Workflow

**1. Triage** → `gh-agent pr view --repo OWNER/REPO N --smart`

Categorizes changes into MECHANICAL (skip), NEW LOGIC (read), BEHAVIORAL (verify inline).

**2. Diffs** → `gh-agent pr diff --repo OWNER/REPO N --smart-files`

Gets diffs for non-mechanical files only. Lock/generated/minified files excluded by default.

**3. Context** — when diffs aren't enough:

```bash
# Read full file at PR branch
gh-agent pr file --repo OWNER/REPO N --path PATH

# Search PR changed files (fast, default)
gh-agent pr grep --repo OWNER/REPO N --pattern "functionName"
gh-agent pr ast-grep --repo OWNER/REPO N --pattern 'useCallback($$$)'

# Search full codebase (only if PR files aren't enough)
gh-agent pr grep --repo OWNER/REPO N --pattern "MyType" --repo-wide
gh-agent pr ast-grep --repo OWNER/REPO N --pattern '$T($$$)' --repo-wide --path src/
```

`--repo-wide` uses GitHub Code Search + always includes PR files at head ref. PR results win on overlap. `--base` searches the base branch instead.

**4. Review** → hand off to `code-review` skill with collected context.

**5. Post** (only when user asks):

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
5. **Search incrementally**. PR files first → `--repo-wide` only if needed.
6. **Post only when asked**. Present findings in chat; user decides when to post.
