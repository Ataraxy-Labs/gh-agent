use crate::diff::{parse_patch, DiffHunk};
use crate::github::{PrFile, PullRequest};

/// Format the metadata header for `pr view`
pub fn format_metadata(pr: &PullRequest) -> String {
    format!(
        "#{} {}  [{}]\n{} ← {}  +{} -{}  {} files",
        pr.number,
        pr.title,
        pr.state,
        pr.base_ref,
        pr.head_ref,
        pr.additions,
        pr.deletions,
        pr.changed_files,
    )
}

/// Format the file stat table
pub fn format_stat_table(files: &[PrFile]) -> String {
    let mut lines = Vec::new();
    for f in files {
        lines.push(format!(
            " {:>9}  {:>+4} {:>-4}  {}",
            f.status,
            f.additions as i64,
            -(f.deletions as i64),
            f.filename,
        ));
    }
    lines.join("\n")
}

/// Format line-numbered unified diff for a single file
pub fn format_line_numbered_diff(file: &PrFile) -> String {
    if file.status == "removed" {
        let total = file.deletions;
        return format!("deleted: {} ({} lines)", file.filename, total);
    }

    let patch = match &file.patch {
        Some(p) if !p.is_empty() => p,
        _ => return format!("--- a/{}\n+++ b/{}\n(no diff)", file.filename, file.filename),
    };

    let hunks = parse_patch(patch);
    let mut out = Vec::new();
    out.push(format!("--- a/{}", file.filename));
    out.push(format!("+++ b/{}", file.filename));

    for hunk in &hunks {
        out.push(format_hunk(hunk));
    }

    out.join("\n")
}

fn format_hunk(hunk: &DiffHunk) -> String {
    let mut lines = Vec::new();
    lines.push(hunk.header.clone());

    for line in &hunk.lines {
        match line.kind.as_str() {
            "add" => {
                let ln = line.new_line.unwrap_or(0);
                lines.push(format!("{:>4} | +{}", ln, line.content));
            }
            "delete" => {
                lines.push(format!("     | -{}", line.content));
            }
            _ => {
                // context
                let ln = line.new_line.unwrap_or(0);
                lines.push(format!("{:>4} |  {}", ln, line.content));
            }
        }
    }

    lines.join("\n")
}
