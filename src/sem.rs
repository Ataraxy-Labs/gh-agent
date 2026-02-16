use anyhow::Result;
use sem_core::git::types::{FileChange, FileStatus};
use sem_core::model::change::{ChangeType, SemanticChange};
use sem_core::parser::differ::{compute_semantic_diff, DiffResult};
use sem_core::parser::plugins::create_default_registry;
use std::collections::{HashMap, HashSet};

// --- Smart analysis types ---

#[derive(Debug, Clone, PartialEq)]
enum ChangeCategory {
    Mechanical,
    NewLogic,
    Behavioral,
}

#[derive(Debug)]
struct CategorizedChange {
    category: ChangeCategory,
    change_type: String,
    entity_type: String,
    entity_name: String,
    file_path: String,
    similarity: f64,
    removed_tokens: Vec<String>,
    added_tokens: Vec<String>,
    value_change: Option<(String, String)>,
}

/// Run sem-core directly on pre-fetched file pairs (no git/CLI needed).
fn run_sem_core(file_pairs: &[(String, String, Option<String>, Option<String>)]) -> DiffResult {
    let file_changes: Vec<FileChange> = file_pairs
        .iter()
        .map(|(filename, status, before, after)| {
            let file_status = match status.as_str() {
                "added" => FileStatus::Added,
                "removed" => FileStatus::Deleted,
                "renamed" => FileStatus::Renamed,
                _ => FileStatus::Modified,
            };
            FileChange {
                file_path: filename.clone(),
                status: file_status,
                old_file_path: None,
                before_content: before.clone(),
                after_content: after.clone(),
            }
        })
        .collect();

    let registry = create_default_registry();
    compute_semantic_diff(&file_changes, &registry, None, None)
}

/// Run sem-core on git refs (requires local git repo + refs fetched).
fn run_sem_core_git(base_ref: &str, head_ref: &str) -> Result<DiffResult> {
    use sem_core::git::bridge::GitBridge;
    use sem_core::git::types::DiffScope;
    use std::path::Path;

    let origin_base = format!("origin/{base_ref}");
    let origin_head = format!("origin/{head_ref}");

    let cwd = std::env::current_dir()?;
    let _git = GitBridge::open(Path::new(&cwd))
        .map_err(|e| anyhow::anyhow!("Not in a git repo: {e}"))?;

    // Use git CLI for merge-base since GitBridge doesn't expose the repo
    let mb_output = std::process::Command::new("git")
        .args(["merge-base", &origin_base, &origin_head])
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run git merge-base: {e}"))?;
    if !mb_output.status.success() {
        anyhow::bail!(
            "Cannot find merge base between {} and {}. Try `git fetch origin` first.",
            origin_base, origin_head
        );
    }
    let merge_base = String::from_utf8_lossy(&mb_output.stdout).trim().to_string();

    let scope = DiffScope::Range {
        from: merge_base,
        to: origin_head,
    };

    let git = GitBridge::open(Path::new(&cwd))
        .map_err(|e| anyhow::anyhow!("Not in a git repo: {e}"))?;
    let file_changes = git.get_changed_files(&scope)
        .map_err(|e| anyhow::anyhow!("Failed to get changed files: {e}"))?;

    let registry = create_default_registry();
    Ok(compute_semantic_diff(&file_changes, &registry, None, None))
}

// --- Formatting ---

fn format_diff_result(result: &DiffResult) -> String {
    let mut lines = Vec::new();

    let mut parts = Vec::new();
    if result.added_count > 0 { parts.push(format!("{} added", result.added_count)); }
    if result.modified_count > 0 { parts.push(format!("{} modified", result.modified_count)); }
    if result.deleted_count > 0 { parts.push(format!("{} deleted", result.deleted_count)); }
    if result.renamed_count > 0 { parts.push(format!("{} renamed", result.renamed_count)); }
    if result.moved_count > 0 { parts.push(format!("{} moved", result.moved_count)); }
    lines.push(format!(
        "Semantic: {} across {} files",
        parts.join(", "),
        result.file_count,
    ));
    lines.push(String::new());

    for c in &result.changes {
        let icon = match c.change_type {
            ChangeType::Added => "⊕",
            ChangeType::Modified => "∆",
            ChangeType::Renamed => "↻",
            ChangeType::Deleted => "⊖",
            ChangeType::Moved => "→",
        };
        let name = if matches!(c.change_type, ChangeType::Moved | ChangeType::Renamed) {
            if let Some(old_path) = &c.old_file_path {
                format!("{} (from {})", c.entity_name, old_path)
            } else {
                c.entity_name.clone()
            }
        } else {
            c.entity_name.clone()
        };
        lines.push(format!(
            "  {} {:<12} {:<35} {}",
            icon, c.entity_type, name, c.file_path
        ));
    }

    lines.join("\n")
}

pub fn run_sem(base_ref: &str, head_ref: &str) -> Result<String> {
    match run_sem_core_git(base_ref, head_ref) {
        Ok(result) => Ok(format_diff_result(&result)),
        Err(e) => Ok(e.to_string()),
    }
}

// --- Smart semantic analysis ---

fn tokenize(s: &str) -> HashSet<String> {
    s.split_whitespace()
        .map(|t| t.to_string())
        .collect()
}

fn jaccard_similarity(before: &str, after: &str) -> f64 {
    let a = tokenize(before);
    let b = tokenize(after);
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let intersection = a.intersection(&b).count();
    let union = a.union(&b).count();
    if union == 0 {
        return 1.0;
    }
    intersection as f64 / union as f64
}

fn token_diff(before: &str, after: &str) -> (Vec<String>, Vec<String>) {
    let a = tokenize(before);
    let b = tokenize(after);
    let removed: Vec<String> = a.difference(&b).cloned().collect();
    let added: Vec<String> = b.difference(&a).cloned().collect();
    (removed, added)
}

fn is_short_value(content: &str) -> bool {
    let trimmed = content.trim();
    trimmed.lines().count() <= 2 && trimmed.len() < 200
}

fn extract_value_change(before: &str, after: &str) -> Option<(String, String)> {
    if !is_short_value(before) || !is_short_value(after) {
        return None;
    }
    let b = before.trim().trim_end_matches(';').trim();
    let a = after.trim().trim_end_matches(';').trim();
    if b != a {
        let extract_rhs = |s: &str| -> String {
            if let Some(pos) = s.find('=') {
                s[pos + 1..].trim().to_string()
            } else {
                s.to_string()
            }
        };
        Some((extract_rhs(b), extract_rhs(a)))
    } else {
        None
    }
}

fn categorize_change(c: &SemanticChange) -> CategorizedChange {
    let ct_str = c.change_type.to_string();

    let (category, similarity, removed_tokens, added_tokens, value_change) =
        match (&c.before_content, &c.after_content) {
            (None, Some(_)) => (ChangeCategory::NewLogic, 0.0, vec![], vec![], None),
            (Some(_), None) => (ChangeCategory::Mechanical, 1.0, vec![], vec![], None),
            (Some(before), Some(after)) => {
                let sim = jaccard_similarity(before, after);
                let (removed, added) = token_diff(before, after);
                let vc = extract_value_change(before, after);

                let cat = if vc.is_some() {
                    ChangeCategory::Behavioral
                } else if sim > 0.8 {
                    ChangeCategory::Mechanical
                } else if sim < 0.5 {
                    ChangeCategory::NewLogic
                } else {
                    ChangeCategory::Behavioral
                };
                (cat, sim, removed, added, vc)
            }
            (None, None) => (ChangeCategory::Mechanical, 1.0, vec![], vec![], None),
        };

    CategorizedChange {
        category,
        change_type: ct_str,
        entity_type: c.entity_type.clone(),
        entity_name: c.entity_name.clone(),
        file_path: c.file_path.clone(),
        similarity,
        removed_tokens,
        added_tokens,
        value_change,
    }
}

fn detect_patterns(changes: &[CategorizedChange]) -> Vec<(String, Vec<usize>)> {
    let mut token_to_indices: HashMap<String, Vec<usize>> = HashMap::new();

    for (i, c) in changes.iter().enumerate() {
        if c.category != ChangeCategory::Mechanical {
            continue;
        }
        for tok in &c.removed_tokens {
            if tok.len() >= 3 {
                token_to_indices.entry(tok.clone()).or_default().push(i);
            }
        }
    }

    let mut patterns: Vec<(String, Vec<usize>)> = token_to_indices
        .into_iter()
        .filter(|(_, indices)| indices.len() >= 2)
        .collect();
    patterns.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    patterns
}

fn short_path(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn format_smart_output(changes: &[SemanticChange], file_count: usize) -> String {
    let categorized: Vec<CategorizedChange> = changes.iter().map(categorize_change).collect();
    let patterns = detect_patterns(&categorized);

    let mut grouped_indices: HashSet<usize> = HashSet::new();
    let mut mechanical_lines: Vec<String> = Vec::new();

    for (token, indices) in &patterns {
        for &idx in indices {
            grouped_indices.insert(idx);
        }
        let fc = indices.len();
        let files: Vec<&str> = indices.iter().map(|&i| short_path(&categorized[i].file_path)).collect();
        let file_list = if files.len() <= 3 {
            files.join(", ")
        } else {
            format!("{} files", fc)
        };
        mechanical_lines.push(format!("  ⊖ {} removed from {}", token, file_list));
    }

    for (i, c) in categorized.iter().enumerate() {
        if c.category != ChangeCategory::Mechanical || grouped_indices.contains(&i) {
            continue;
        }
        let icon = match c.change_type.as_str() {
            "deleted" => "⊖",
            "renamed" => "↻",
            _ => "∆",
        };
        let desc = if c.change_type == "deleted" {
            format!("  {} {} {} — deleted", icon, short_path(&c.file_path), c.entity_name)
        } else if !c.removed_tokens.is_empty() || !c.added_tokens.is_empty() {
            let mut parts = Vec::new();
            if !c.removed_tokens.is_empty() {
                let top: Vec<&str> = c.removed_tokens.iter().take(3).map(|s| s.as_str()).collect();
                parts.push(format!("-{}", top.join(",")));
            }
            if !c.added_tokens.is_empty() {
                let top: Vec<&str> = c.added_tokens.iter().take(3).map(|s| s.as_str()).collect();
                parts.push(format!("+{}", top.join(",")));
            }
            format!(
                "  {} {:<20} {:<30} ({})",
                icon, short_path(&c.file_path), c.entity_name, parts.join(" "),
            )
        } else {
            format!(
                "  {} {:<20} {} (sim {:.0}%)",
                icon, short_path(&c.file_path), c.entity_name, c.similarity * 100.0,
            )
        };
        mechanical_lines.push(desc);
    }

    let mut new_logic_lines: Vec<String> = Vec::new();
    for c in &categorized {
        if c.category != ChangeCategory::NewLogic { continue; }
        new_logic_lines.push(format!(
            "  ⊕ {:<20} {} — {}",
            short_path(&c.file_path), c.entity_name, c.entity_type,
        ));
    }

    let mut behavioral_lines: Vec<String> = Vec::new();
    for c in &categorized {
        if c.category != ChangeCategory::Behavioral { continue; }
        let detail = if let Some((old_val, new_val)) = &c.value_change {
            format!("{} → {}", old_val, new_val)
        } else if !c.removed_tokens.is_empty() || !c.added_tokens.is_empty() {
            let mut parts = Vec::new();
            if !c.removed_tokens.is_empty() {
                let top: Vec<&str> = c.removed_tokens.iter().take(3).map(|s| s.as_str()).collect();
                parts.push(format!("-{}", top.join(",")));
            }
            if !c.added_tokens.is_empty() {
                let top: Vec<&str> = c.added_tokens.iter().take(3).map(|s| s.as_str()).collect();
                parts.push(format!("+{}", top.join(",")));
            }
            parts.join(" ")
        } else {
            format!("sim {:.0}%", c.similarity * 100.0)
        };
        behavioral_lines.push(format!(
            "  ∆ {:<20} {:<30} {}",
            short_path(&c.file_path), c.entity_name, detail,
        ));
    }

    let mut out = Vec::new();

    out.push(format!(
        "Smart Review: {} changes across {} files\n",
        changes.len(), file_count,
    ));

    if !mechanical_lines.is_empty() {
        out.push(format!(
            "MECHANICAL (skip — {} changes):",
            categorized.iter().filter(|c| c.category == ChangeCategory::Mechanical).count()
        ));
        out.extend(mechanical_lines);
        out.push(String::new());
    }

    if !new_logic_lines.is_empty() {
        out.push(format!(
            "NEW LOGIC (read these — {} changes):",
            categorized.iter().filter(|c| c.category == ChangeCategory::NewLogic).count()
        ));
        out.extend(new_logic_lines);
        out.push(String::new());
    }

    if !behavioral_lines.is_empty() {
        out.push(format!(
            "BEHAVIORAL CHANGES (verify — {} changes):",
            categorized.iter().filter(|c| c.category == ChangeCategory::Behavioral).count()
        ));
        out.extend(behavioral_lines);
        out.push(String::new());
    }

    out.join("\n")
}

/// Smart review from pre-fetched file pairs (no git/CWD needed)
pub fn run_sem_smart_from_pairs(
    file_pairs: &[(String, String, Option<String>, Option<String>)],
) -> Result<String> {
    if file_pairs.is_empty() {
        return Ok("No files to analyze.".to_string());
    }

    let result = run_sem_core(file_pairs);

    if result.changes.is_empty() {
        return Ok("No semantic changes found.".to_string());
    }

    Ok(format_smart_output(&result.changes, result.file_count))
}

/// Returns deduplicated file paths for non-mechanical changes from pre-fetched pairs.
pub fn get_smart_files_from_pairs(
    file_pairs: &[(String, String, Option<String>, Option<String>)],
) -> Option<Vec<String>> {
    let result = run_sem_core(file_pairs);
    let categorized: Vec<CategorizedChange> = result.changes.iter().map(categorize_change).collect();

    let mut files: Vec<String> = categorized
        .iter()
        .filter(|c| c.category != ChangeCategory::Mechanical)
        .map(|c| c.file_path.clone())
        .collect();
    files.sort();
    files.dedup();
    Some(files)
}
