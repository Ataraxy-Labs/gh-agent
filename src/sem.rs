use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::process::Command;

/// Resolve the sem binary: respect `SEM_BIN` env var, otherwise look for `sem` on PATH.
fn sem_bin() -> String {
    std::env::var("SEM_BIN").unwrap_or_else(|_| "sem".to_string())
}

#[derive(Debug, Deserialize)]
struct SemOutput {
    summary: Option<SemSummary>,
    changes: Option<Vec<SemChange>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SemSummary {
    #[serde(default)]
    added: u64,
    #[serde(default)]
    modified: u64,
    #[serde(default)]
    deleted: u64,
    #[serde(default)]
    renamed: u64,
    #[serde(default)]
    moved: u64,
    #[serde(default)]
    file_count: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SemChange {
    change_type: String,
    entity_type: String,
    entity_name: String,
    file_path: String,
    old_file_path: Option<String>,
    before_content: Option<String>,
    after_content: Option<String>,
}

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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SemFileInput {
    file_path: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    old_file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    before_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    after_content: Option<String>,
}

/// Find the merge base between two refs
fn git_merge_base(base_ref: &str, head_ref: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["merge-base", base_ref, head_ref])
        .output()
        .map_err(|e| format!("Failed to run git merge-base: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "Cannot find merge base between {base_ref} and {head_ref}. Try `git fetch origin {base_ref} {head_ref}` first."
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Check if we're in a git repo and refs exist
fn check_git_refs(base_ref: &str, head_ref: &str) -> Result<(), String> {
    let git_check = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .output();

    match git_check {
        Ok(out) if out.status.success() => {}
        _ => return Err("Cannot run semantic analysis: not inside a git repository.".to_string()),
    }

    for r in [base_ref, head_ref] {
        let check = Command::new("git")
            .args(["rev-parse", "--verify", r])
            .output();
        match check {
            Ok(out) if out.status.success() => {}
            _ => return Err(format!(
                "Cannot run semantic analysis: ref {r} not available locally. Try `git fetch origin` first."
            )),
        }
    }

    Ok(())
}

pub fn run_sem(base_ref: &str, head_ref: &str) -> Result<String> {
    let origin_base = format!("origin/{base_ref}");
    let origin_head = format!("origin/{head_ref}");

    if let Err(msg) = check_git_refs(&origin_base, &origin_head) {
        return Ok(msg);
    }

    // Use merge-base to scope to only PR changes
    let merge_base = match git_merge_base(&origin_base, &origin_head) {
        Ok(mb) => mb,
        Err(msg) => return Ok(msg),
    };

    let output = Command::new(sem_bin())
        .arg("diff")
        .arg("--from")
        .arg(&merge_base)
        .arg("--to")
        .arg(&origin_head)
        .arg("--format")
        .arg("json")
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Ok(format!("Semantic analysis failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: SemOutput = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(e) => return Ok(format!("Failed to parse sem output: {e}")),
    };

    let mut lines = Vec::new();

    if let Some(s) = &parsed.summary {
        let mut parts = Vec::new();
        if s.added > 0 { parts.push(format!("{} added", s.added)); }
        if s.modified > 0 { parts.push(format!("{} modified", s.modified)); }
        if s.deleted > 0 { parts.push(format!("{} deleted", s.deleted)); }
        if s.renamed > 0 { parts.push(format!("{} renamed", s.renamed)); }
        if s.moved > 0 { parts.push(format!("{} moved", s.moved)); }
        lines.push(format!(
            "Semantic: {} across {} files",
            parts.join(", "),
            s.file_count,
        ));
        lines.push(String::new());
    }

    if let Some(changes) = &parsed.changes {
        for c in changes {
            let icon = match c.change_type.as_str() {
                "added" => "⊕",
                "modified" => "∆",
                "renamed" => "↻",
                "deleted" => "⊖",
                "moved" => "→",
                _ => "?",
            };
            let name = if c.change_type == "moved" || c.change_type == "renamed" {
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
    }

    Ok(lines.join("\n"))
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

fn categorize_change(c: &SemChange) -> CategorizedChange {
    let (category, similarity, removed_tokens, added_tokens, value_change) =
        match (&c.before_content, &c.after_content) {
            // New entity — no before
            (None, Some(_)) => (ChangeCategory::NewLogic, 0.0, vec![], vec![], None),
            // Deleted entity — no after
            (Some(_), None) => (ChangeCategory::Mechanical, 1.0, vec![], vec![], None),
            // Both present — compare
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
            // Neither — shouldn't happen, treat as mechanical
            (None, None) => (ChangeCategory::Mechanical, 1.0, vec![], vec![], None),
        };

    CategorizedChange {
        category,
        change_type: c.change_type.clone(),
        entity_type: c.entity_type.clone(),
        entity_name: c.entity_name.clone(),
        file_path: c.file_path.clone(),
        similarity,
        removed_tokens,
        added_tokens,
        value_change,
    }
}

/// Detect common patterns: tokens removed/added across multiple entities
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

/// Run sem diff via stdin with pre-fetched file contents
fn run_sem_stdin(file_inputs: &[SemFileInput]) -> Result<SemOutput> {
    let json_input = serde_json::to_string(file_inputs)?;

    let mut child = Command::new(sem_bin())
        .arg("diff")
        .arg("--stdin")
        .arg("--format")
        .arg("json")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(json_input.as_bytes())?;
    }

    let output = child.wait_with_output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("sem stdin failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: SemOutput = serde_json::from_str(&stdout)?;
    Ok(parsed)
}

fn format_smart_output(parsed: &SemOutput) -> String {
    let changes = match &parsed.changes {
        Some(c) => c,
        None => return "No semantic changes found.".to_string(),
    };

    let categorized: Vec<CategorizedChange> = changes.iter().map(categorize_change).collect();
    let patterns = detect_patterns(&categorized);

    let mut grouped_indices: HashSet<usize> = HashSet::new();
    let mut mechanical_lines: Vec<String> = Vec::new();

    for (token, indices) in &patterns {
        for &idx in indices {
            grouped_indices.insert(idx);
        }
        let file_count = indices.len();
        let files: Vec<&str> = indices.iter().map(|&i| short_path(&categorized[i].file_path)).collect();
        let file_list = if files.len() <= 3 {
            files.join(", ")
        } else {
            format!("{} files", file_count)
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
        let icon = match c.change_type.as_str() {
            "added" => "⊕",
            _ => "⊕",
        };
        new_logic_lines.push(format!(
            "  {} {:<20} {} — {}",
            icon, short_path(&c.file_path), c.entity_name, c.entity_type,
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

    if let Some(s) = &parsed.summary {
        out.push(format!(
            "Smart Review: {} changes across {} files\n",
            changes.len(), s.file_count,
        ));
    }

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
    let file_inputs: Vec<SemFileInput> = file_pairs
        .iter()
        .map(|(filename, status, before, after)| {
            let sem_status = match status.as_str() {
                "added" => "added",
                "removed" => "deleted",
                "renamed" => "renamed",
                _ => "modified",
            };
            SemFileInput {
                file_path: filename.clone(),
                status: sem_status.to_string(),
                old_file_path: None,
                before_content: before.clone(),
                after_content: after.clone(),
            }
        })
        .collect();

    if file_inputs.is_empty() {
        return Ok("No files to analyze.".to_string());
    }

    let parsed = match run_sem_stdin(&file_inputs) {
        Ok(p) => p,
        Err(e) => return Ok(format!("Smart analysis failed: {e}")),
    };

    Ok(format_smart_output(&parsed))
}

/// Returns deduplicated file paths for non-mechanical changes from pre-fetched pairs.
/// Returns None if sem fails (caller should fall back to all files).
pub fn get_smart_files_from_pairs(
    file_pairs: &[(String, String, Option<String>, Option<String>)],
) -> Option<Vec<String>> {
    let file_inputs: Vec<SemFileInput> = file_pairs
        .iter()
        .map(|(filename, status, before, after)| {
            let sem_status = match status.as_str() {
                "added" => "added",
                "removed" => "deleted",
                "renamed" => "renamed",
                _ => "modified",
            };
            SemFileInput {
                file_path: filename.clone(),
                status: sem_status.to_string(),
                old_file_path: None,
                before_content: before.clone(),
                after_content: after.clone(),
            }
        })
        .collect();

    let parsed = run_sem_stdin(&file_inputs).ok()?;
    let changes = parsed.changes.as_ref()?;
    let categorized: Vec<CategorizedChange> = changes.iter().map(categorize_change).collect();

    let mut files: Vec<String> = categorized
        .iter()
        .filter(|c| c.category != ChangeCategory::Mechanical)
        .map(|c| c.file_path.clone())
        .collect();
    files.sort();
    files.dedup();
    Some(files)
}
