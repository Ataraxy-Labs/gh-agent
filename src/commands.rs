use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::diff::{commentable_lines, parse_patch};
use crate::format;
use crate::github::{self, CreateReview, ReviewCommentInput};
use crate::search;
use crate::sem;

// --- Output types for JSON ---

#[derive(Serialize)]
struct PrViewJson {
    number: u64,
    title: String,
    body: Option<String>,
    state: String,
    head_sha: String,
    head_ref: String,
    base_ref: String,
    additions: u64,
    deletions: u64,
    changed_files: u64,
    files: Vec<FileStatJson>,
}

#[derive(Serialize)]
struct FileStatJson {
    path: String,
    status: String,
    additions: u64,
    deletions: u64,
}

#[derive(Serialize)]
struct DiffJson {
    files: HashMap<String, Vec<u64>>,
}

#[derive(Serialize)]
struct FileOut {
    path: String,
    content: String,
    lines: usize,
}

#[derive(Serialize)]
struct ReviewOut {
    id: u64,
    url: String,
}

#[derive(Deserialize)]
struct CommentInput {
    path: String,
    line: u64,
    body: String,
    #[serde(default)]
    start_line: Option<u64>,
}

#[derive(Deserialize)]
struct ReviewInput {
    #[serde(default = "default_body")]
    body: String,
    comments: Vec<CommentInput>,
}

fn default_body() -> String {
    "Review from gh-agent".to_string()
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

// --- Noise file filtering ---

/// Files that are never useful in a code review diff.
/// Excluded by default; pass --all to include them.
const NOISE_EXACT: &[&str] = &[
    // JS/TS
    "pnpm-lock.yaml",
    "package-lock.json",
    "yarn.lock",
    "npm-shrinkwrap.json",
    "bun.lockb",
    // Rust
    "Cargo.lock",
    // Ruby
    "Gemfile.lock",
    // Python
    "poetry.lock",
    "Pipfile.lock",
    "uv.lock",
    // Go
    "go.sum",
    // PHP
    "composer.lock",
    // .NET
    "packages.lock.json",
    // Dart/Flutter
    "pubspec.lock",
    // Swift
    "Package.resolved",
    // Elixir
    "mix.lock",
    // Misc
    ".DS_Store",
];

const NOISE_EXTENSIONS: &[&str] = &[
    ".min.js",
    ".min.css",
    ".map",
    ".chunk.js",
    ".bundle.js",
];

const NOISE_PREFIXES: &[&str] = &[
    "dist/",
    ".next/",
    "build/",
    "__generated__/",
    ".turbo/",
];

pub(crate) fn is_noise_file(path: &str) -> bool {
    let filename = path.rsplit('/').next().unwrap_or(path);

    if NOISE_EXACT.iter().any(|n| filename == *n) {
        return true;
    }

    if NOISE_EXTENSIONS.iter().any(|ext| path.ends_with(ext)) {
        return true;
    }

    if NOISE_PREFIXES.iter().any(|prefix| path.starts_with(prefix)) {
        return true;
    }

    false
}

// --- Commands ---

pub async fn pr_view(
    client: &github::Client,
    repo: &str,
    number: u64,
    use_sem: bool,
    use_smart: bool,
    json: bool,
) -> Result<()> {
    let pr = client.get_pr(repo, number).await?;

    if json {
        let out = PrViewJson {
            number: pr.number,
            title: pr.title.clone(),
            body: pr.body.clone(),
            state: pr.state.clone(),
            head_sha: pr.head_sha.clone(),
            head_ref: pr.head_ref.clone(),
            base_ref: pr.base_ref.clone(),
            additions: pr.additions,
            deletions: pr.deletions,
            changed_files: pr.changed_files,
            files: pr
                .files
                .iter()
                .map(|f| FileStatJson {
                    path: f.filename.clone(),
                    status: f.status.clone(),
                    additions: f.additions,
                    deletions: f.deletions,
                })
                .collect(),
        };
        return print_json(&out);
    }

    let noise_count = pr.files.iter().filter(|f| is_noise_file(&f.filename)).count();
    let visible_files: Vec<github::PrFile> = pr
        .files
        .iter()
        .filter(|f| !is_noise_file(&f.filename))
        .cloned()
        .collect();

    println!("{}", format::format_metadata(&pr));
    println!();
    println!("{}", format::format_stat_table(&visible_files));
    if noise_count > 0 {
        eprintln!("({} noise files hidden: lock/generated/minified)", noise_count);
    }

    if use_smart {
        println!();
        eprintln!("smart: fetching file contents from GitHub API...");
        let pairs = client
            .get_file_pairs(repo, &visible_files, &pr.base_ref, &pr.head_ref)
            .await;
        let smart_output = sem::run_sem_smart_from_pairs(&pairs)?;
        println!("{smart_output}");
    } else if use_sem {
        println!();
        let sem_output = sem::run_sem(&pr.base_ref, &pr.head_ref)?;
        println!("{sem_output}");
    }

    Ok(())
}

pub async fn pr_diff(
    client: &github::Client,
    repo: &str,
    number: u64,
    file_filters: &[String],
    smart_files: bool,
    include_all: bool,
    stat_only: bool,
    json: bool,
) -> Result<()> {
    let pr = client.get_pr_with_patches(repo, number).await?;

    // Build the file filter list: --smart-files fetches contents from API, runs sem, filters
    let smart_list = if smart_files {
        eprintln!("smart: fetching file contents from GitHub API...");
        let pairs = client
            .get_file_pairs(repo, &pr.files, &pr.base_ref, &pr.head_ref)
            .await;
        match sem::get_smart_files_from_pairs(&pairs) {
            Some(sf) => {
                eprintln!("smart: filtering to {} files (skipped mechanical)", sf.len());
                sf
            }
            None => {
                eprintln!("smart: sem analysis failed, showing all files");
                vec![]
            }
        }
    } else {
        vec![]
    };

    let files: Vec<&github::PrFile> = if !file_filters.is_empty() {
        // Explicit --file flags: substring match
        pr.files
            .iter()
            .filter(|f| file_filters.iter().any(|filter| f.filename.contains(filter.as_str())))
            .collect()
    } else if smart_files && !smart_list.is_empty() {
        // --smart-files with successful sem: exact path match
        pr.files
            .iter()
            .filter(|f| smart_list.iter().any(|sf| f.filename == *sf))
            .collect()
    } else {
        // No filter or sem fallback: all files
        pr.files.iter().collect()
    };

    // Apply noise filter unless --all is set
    let (files, skipped) = if include_all {
        (files, 0usize)
    } else {
        let before = files.len();
        let filtered: Vec<&github::PrFile> = files
            .into_iter()
            .filter(|f| !is_noise_file(&f.filename))
            .collect();
        let skipped = before - filtered.len();
        (filtered, skipped)
    };

    if skipped > 0 {
        eprintln!("skipped {} noise files (lock/generated/minified). Use --all to include.", skipped);
    }

    if json {
        let mut map = HashMap::new();
        for f in &files {
            let hunks = f.patch.as_deref().map(parse_patch).unwrap_or_default();
            let cl = commentable_lines(&hunks);
            map.insert(f.filename.clone(), cl);
        }
        return print_json(&DiffJson { files: map });
    }

    if stat_only {
        let borrowed: Vec<github::PrFile> = files.iter().map(|f| (*f).clone()).collect();
        println!("{}", format::format_stat_table(&borrowed));
        return Ok(());
    }

    for (i, f) in files.iter().enumerate() {
        if i > 0 {
            println!();
        }
        println!("{}", format::format_line_numbered_diff(f));
    }

    Ok(())
}

pub async fn pr_file(
    client: &github::Client,
    repo: &str,
    number: u64,
    path: &str,
) -> Result<()> {
    let pr = client.get_pr(repo, number).await?;
    let content = client
        .get_file_content(repo, path, &pr.head_ref)
        .await?;
    let lines = content.lines().count();

    let out = FileOut {
        path: path.to_string(),
        content,
        lines,
    };
    print_json(&out)
}

pub async fn pr_review(
    client: &github::Client,
    repo: &str,
    number: u64,
    comments_file: &str,
) -> Result<()> {
    let pr = client.get_pr_with_patches(repo, number).await?;

    let file_commentable: HashMap<String, Vec<u64>> = pr
        .files
        .iter()
        .map(|f| {
            let hunks = f.patch.as_deref().map(parse_patch).unwrap_or_default();
            let cl = commentable_lines(&hunks);
            (f.filename.clone(), cl)
        })
        .collect();

    let raw = std::fs::read_to_string(comments_file)
        .with_context(|| format!("Failed to read {comments_file}"))?;
    let input: ReviewInput = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse {comments_file}"))?;

    let mut warnings = Vec::new();
    let mut valid_comments = Vec::new();

    for c in &input.comments {
        if let Some(cl) = file_commentable.get(&c.path) {
            if cl.contains(&c.line) {
                valid_comments.push(ReviewCommentInput {
                    path: c.path.clone(),
                    line: c.line,
                    body: c.body.clone(),
                    start_line: c.start_line,
                });
            } else {
                warnings.push(format!(
                    "SKIP: {}:{} is not a commentable line (not in diff)",
                    c.path, c.line
                ));
            }
        } else {
            warnings.push(format!(
                "SKIP: {} is not a changed file in this PR",
                c.path
            ));
        }
    }

    if !warnings.is_empty() {
        eprintln!("⚠️  Validation warnings:");
        for w in &warnings {
            eprintln!("  {w}");
        }
    }

    if valid_comments.is_empty() {
        anyhow::bail!("No valid comments to post after validation");
    }

    let review = CreateReview {
        commit_id: pr.head_sha,
        event: "COMMENT".to_string(),
        body: input.body,
        comments: valid_comments,
    };

    let resp = client.create_review(repo, number, &review).await?;

    let out = ReviewOut {
        id: resp.id,
        url: resp.html_url,
    };
    print_json(&out)
}

pub async fn pr_suggest(
    client: &github::Client,
    repo: &str,
    number: u64,
    file: &str,
    line_start: u64,
    line_end: u64,
    replacement: &str,
) -> Result<()> {
    let pr = client.get_pr(repo, number).await?;

    let body = format!("```suggestion\n{replacement}\n```");

    let start_line = if line_start == line_end {
        None
    } else {
        Some(line_start)
    };

    let review = CreateReview {
        commit_id: pr.head_sha,
        event: "COMMENT".to_string(),
        body: "Suggestion from gh-agent".to_string(),
        comments: vec![ReviewCommentInput {
            path: file.to_string(),
            line: line_end,
            body,
            start_line,
        }],
    };

    let resp = client.create_review(repo, number, &review).await?;
    let out = ReviewOut {
        id: resp.id,
        url: resp.html_url,
    };
    print_json(&out)
}

/// Extract a text keyword from an ast-grep pattern for pre-filtering via code search.
/// Takes everything before the first meta-variable ($) or opening paren with $.
/// Falls back to the whole pattern if no good keyword found.
fn extract_search_keyword(pattern: &str) -> &str {
    let end = pattern.find('$').unwrap_or(pattern.len());
    let keyword = pattern[..end].trim().trim_end_matches('(');
    if keyword.is_empty() {
        pattern.split_whitespace().next().unwrap_or(pattern)
    } else {
        keyword
    }
}

pub async fn pr_grep(
    client: &github::Client,
    repo: &str,
    number: u64,
    pattern: &str,
    file_filters: &[String],
    repo_wide: bool,
    path_prefix: Option<&str>,
    use_base: bool,
    case_sensitive: bool,
    context_lines: usize,
    include_all: bool,
) -> Result<()> {
    let pr = client.get_pr(repo, number).await?;
    let git_ref = if use_base { &pr.base_ref } else { &pr.head_ref };

    // Always search PR changed files at correct ref
    let mut pr_file_paths: Vec<String> = pr.files.iter().map(|f| f.filename.clone()).collect();
    if !file_filters.is_empty() {
        pr_file_paths.retain(|p| file_filters.iter().any(|f| p.contains(f.as_str())));
    }
    if !include_all {
        pr_file_paths.retain(|p| !is_noise_file(p));
    }

    eprintln!("Fetching {} PR files at {}...", pr_file_paths.len(), git_ref);
    let pr_files = fetch_file_contents(client, repo, &pr_file_paths, git_ref).await;
    let mut pr_matches = search::grep_files(&pr_files, pattern, case_sensitive, context_lines);

    if repo_wide {
        // Search the broader codebase via GitHub Code Search (default branch)
        eprintln!("Searching codebase via GitHub Code Search...");
        let search_results = client.search_code(repo, pattern, path_prefix).await?;
        eprintln!("Code Search: {} results from default branch", search_results.total_count);

        // Convert code search results to SearchMatch, but skip files already in PR
        let pr_file_set: std::collections::HashSet<&str> = pr_file_paths.iter().map(|s| s.as_str()).collect();

        for item in &search_results.items {
            if pr_file_set.contains(item.path.as_str()) {
                continue; // PR version takes priority
            }
            if !include_all && is_noise_file(&item.path) {
                continue;
            }
            if let Some(text_matches) = &item.text_matches {
                for tm in text_matches {
                    for (line_idx, line) in tm.fragment.lines().enumerate() {
                        let haystack = if case_sensitive { line.to_string() } else { line.to_lowercase() };
                        let pat = if case_sensitive { pattern.to_string() } else { pattern.to_lowercase() };
                        if haystack.contains(&pat) {
                            pr_matches.push(search::SearchMatch {
                                file: item.path.clone(),
                                line: line_idx + 1,
                                column: haystack.find(&pat).unwrap_or(0) + 1,
                                text: line.to_string(),
                                context_before: vec![],
                                context_after: vec![],
                            });
                        }
                    }
                }
            }
        }
    }

    println!("{}", search::format_matches(&pr_matches));
    Ok(())
}

pub async fn pr_ast_grep(
    client: &github::Client,
    repo: &str,
    number: u64,
    pattern: &str,
    file_filters: &[String],
    repo_wide: bool,
    path_prefix: Option<&str>,
    use_base: bool,
    lang_override: Option<&str>,
    include_all: bool,
) -> Result<()> {
    let pr = client.get_pr(repo, number).await?;
    let git_ref = if use_base { &pr.base_ref } else { &pr.head_ref };

    let lang: Option<ast_grep_language::SupportLang> = lang_override
        .map(|l| l.parse())
        .transpose()
        .map_err(|e: ast_grep_language::SupportLangErr| anyhow::anyhow!("{e}"))
        .context("Invalid language. Use: ts, tsx, js, jsx, py, rs, go, java, etc.")?;

    // Collect PR changed file paths
    let mut pr_file_paths: Vec<String> = pr.files.iter().map(|f| f.filename.clone()).collect();
    if !file_filters.is_empty() {
        pr_file_paths.retain(|p| file_filters.iter().any(|f| p.contains(f.as_str())));
    }
    if !include_all {
        pr_file_paths.retain(|p| !is_noise_file(p));
    }

    let mut all_file_paths = pr_file_paths.clone();

    if repo_wide {
        // Use text keyword from AST pattern to pre-filter via Code Search
        let keyword = extract_search_keyword(pattern);
        eprintln!("Searching codebase for '{}' via GitHub Code Search...", keyword);

        let search_results = client.search_code(repo, keyword, path_prefix).await?;
        eprintln!("Code Search: {} candidate files from default branch", search_results.total_count);

        let pr_file_set: std::collections::HashSet<String> = pr_file_paths.iter().cloned().collect();

        for item in &search_results.items {
            if !pr_file_set.contains(&item.path) {
                if include_all || !is_noise_file(&item.path) {
                    all_file_paths.push(item.path.clone());
                }
            }
        }

        // Dedup
        all_file_paths.sort();
        all_file_paths.dedup();
    }

    if all_file_paths.is_empty() {
        println!("No files to search.");
        return Ok(());
    }

    eprintln!("Fetching {} files at {}...", all_file_paths.len(), git_ref);
    let files = fetch_file_contents(client, repo, &all_file_paths, git_ref).await;

    if files.is_empty() {
        println!("No readable files found.");
        return Ok(());
    }

    let matches = search::ast_grep_files(&files, pattern, lang)?;
    println!("{}", search::format_matches(&matches));
    Ok(())
}

/// Fetch file contents concurrently, skipping failures silently
async fn fetch_file_contents(
    client: &github::Client,
    repo: &str,
    paths: &[String],
    git_ref: &str,
) -> Vec<(String, String)> {
    let futs: Vec<_> = paths
        .iter()
        .map(|path| {
            let path = path.clone();
            let repo = repo.to_string();
            let git_ref = git_ref.to_string();
            async move {
                match client.get_file_content(&repo, &path, &git_ref).await {
                    Ok(content) => Some((path, content)),
                    Err(_) => None, // skip binary/too-large/404
                }
            }
        })
        .collect();

    futures::future::join_all(futs)
        .await
        .into_iter()
        .flatten()
        .collect()
}
