use anyhow::{Context, Result};
use ast_grep_core::Pattern;
use ast_grep_language::{LanguageExt, SupportLang};

/// Result of a single match
pub struct SearchMatch {
    pub file: String,
    pub line: usize,     // 1-indexed
    pub column: usize,   // 1-indexed
    pub text: String,     // the matched line (for grep) or matched node text (for ast-grep)
    pub context_before: Vec<String>,
    pub context_after: Vec<String>,
}

/// Text grep across fetched file contents
/// files: Vec of (filepath, content)
/// Returns matches in grep-style format
pub fn grep_files(
    files: &[(String, String)],
    pattern: &str,
    case_sensitive: bool,
    context_lines: usize,
) -> Vec<SearchMatch> {
    let pattern_lower = if case_sensitive { pattern.to_string() } else { pattern.to_lowercase() };
    let mut matches = Vec::new();

    for (filepath, content) in files {
        let lines: Vec<&str> = content.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let haystack = if case_sensitive { line.to_string() } else { line.to_lowercase() };
            if haystack.contains(&pattern_lower) {
                let start = i.saturating_sub(context_lines);
                let end = (i + context_lines + 1).min(lines.len());
                matches.push(SearchMatch {
                    file: filepath.clone(),
                    line: i + 1,
                    column: haystack.find(&pattern_lower).unwrap_or(0) + 1,
                    text: line.to_string(),
                    context_before: lines[start..i].iter().map(|s| s.to_string()).collect(),
                    context_after: lines[i+1..end].iter().map(|s| s.to_string()).collect(),
                });
            }
        }
    }
    matches
}

/// Infer SupportLang from file extension
pub fn lang_from_path(path: &str) -> Option<SupportLang> {
    let ext = path.rsplit('.').next()?;
    // SupportLang::from_str accepts extensions like "ts", "tsx", "py", "rs", etc.
    ext.parse().ok()
}

/// AST-grep structural search across fetched file contents
/// files: Vec of (filepath, content)
/// pattern: ast-grep pattern string like "console.log($$$)"
/// lang_override: if set, use this lang for all files; otherwise infer from extension
pub fn ast_grep_files(
    files: &[(String, String)],
    pattern: &str,
    lang_override: Option<SupportLang>,
) -> Result<Vec<SearchMatch>> {
    let mut matches = Vec::new();

    for (filepath, content) in files {
        let lang = lang_override
            .or_else(|| lang_from_path(filepath));

        let lang = match lang {
            Some(l) => l,
            None => continue, // skip files with unrecognized extensions
        };

        // Parse the pattern for this language
        let pat = Pattern::try_new(pattern, lang)
            .with_context(|| format!("Invalid ast-grep pattern for language {lang}"))?;

        let root = lang.ast_grep(content);
        let lines: Vec<&str> = content.lines().collect();
        let _ = &lines; // suppress unused warning if no matches

        for node_match in root.root().find_all(&pat) {
            let start = node_match.start_pos();
            let line_num = start.line(); // 0-indexed
            let col = start.column(&*node_match); // 0-indexed
            let matched_text = node_match.text().to_string();

            matches.push(SearchMatch {
                file: filepath.clone(),
                line: line_num + 1,
                column: col + 1,
                text: matched_text,
                context_before: vec![],
                context_after: vec![],
            });
        }
    }

    Ok(matches)
}

/// Format search matches for terminal output (grep-style)
pub fn format_matches(matches: &[SearchMatch]) -> String {
    if matches.is_empty() {
        return "No matches found.".to_string();
    }

    let mut lines = Vec::new();
    let mut last_file = "";

    for m in matches {
        if m.file != last_file {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            last_file = &m.file;
        }

        // Context before
        for (j, ctx) in m.context_before.iter().enumerate() {
            let ctx_line = m.line - m.context_before.len() + j;
            lines.push(format!("{}:{}- {}", m.file, ctx_line, ctx));
        }

        // The match itself
        lines.push(format!("{}:{}:{}", m.file, m.line, m.text));

        // Context after
        for (j, ctx) in m.context_after.iter().enumerate() {
            lines.push(format!("{}:{}- {}", m.file, m.line + 1 + j, ctx));
        }
    }

    lines.push(format!("\n{} matches across {} files",
        matches.len(),
        {
            let mut files: Vec<&str> = matches.iter().map(|m| m.file.as_str()).collect();
            files.sort();
            files.dedup();
            files.len()
        }
    ));

    lines.join("\n")
}
