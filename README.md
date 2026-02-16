# gh-agent

Agent-friendly GitHub CLI for PR reviews. Designed to be used by AI coding agents to intelligently triage, search, and review pull requests — all via the GitHub API (no local clone needed).

## Install

### Homebrew (recommended)

```bash
brew install ataraxy-labs/tap/gh-agent
```

### From source

```bash
git clone https://github.com/Ataraxy-Labs/gh-agent.git
cd gh-agent
cargo install --path .
```

## Requirements

- **GitHub token**: Set `GITHUB_TOKEN` env var, or have the [GitHub CLI](https://cli.github.com/) installed and authenticated (`gh auth login`).
- **sem** is bundled — semantic analysis works out of the box, no separate install needed.

## Usage

### Quick start

```bash
# Smart triage — always start here
gh-agent pr view --repo owner/repo 123 --smart

# Diffs for non-mechanical files only
gh-agent pr diff --repo owner/repo 123 --smart-files

# Read a file at the PR branch
gh-agent pr file --repo owner/repo 123 --path src/main.rs

# Search PR changed files
gh-agent pr grep --repo owner/repo 123 --pattern "functionName"

# Structural search (AST)
gh-agent pr ast-grep --repo owner/repo 123 --pattern 'useCallback($$$)'

# Search full codebase (GitHub Code Search + PR files)
gh-agent pr grep --repo owner/repo 123 --pattern "MyType" --repo-wide
```

### Commands

| Command | Purpose |
|---|---|
| `pr view --repo R N --smart` | Smart triage — categorizes changes |
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

### Smart triage

The `--smart` flag uses semantic analysis to categorize every change in the PR:

- **MECHANICAL** — safe to skip (renames, import reorders, formatting)
- **NEW LOGIC** — new code that needs reading
- **BEHAVIORAL** — existing logic that changed (verify these)

### Posting reviews

```bash
# Check which lines are commentable
gh-agent pr diff --repo owner/repo 123 --json

# Post review comments from a JSON file
gh-agent pr review --repo owner/repo 123 --comments-file review.json

# Post a suggestion
gh-agent pr suggest --repo owner/repo 123 \
  --file src/main.rs --line-start 10 --line-end 12 \
  --replacement "new code here"
```

Review JSON format:

```json
{
  "body": "Review summary",
  "comments": [
    { "path": "src/main.rs", "line": 42, "body": "Consider error handling here" },
    { "path": "src/lib.rs", "line": 10, "body": "suggestion text", "start_line": 8 }
  ]
}
```

## Agent skill

This repo includes an agent skill that teaches AI coding agents the full gh-agent PR review workflow.

```bash
npx skills add ataraxy-labs/gh-agent
```

Or browse it at [skills.sh/ataraxy-labs/gh-agent](https://skills.sh/ataraxy-labs/gh-agent).

## License

MIT
