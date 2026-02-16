use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "gh-agent", about = "Agent-friendly GitHub CLI for PR reviews")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Pull request operations
    Pr {
        #[command(subcommand)]
        command: PrCommands,
    },
}

#[derive(Subcommand)]
pub enum PrCommands {
    /// One-stop PR overview: metadata, file stats, optional semantic summary
    View {
        /// PR number
        number: u64,
        /// Repository in owner/repo format
        #[arg(short, long)]
        repo: String,
        /// Run semantic analysis via sem
        #[arg(long)]
        sem: bool,
        /// Smart categorized review guide (uses sem beforeContent/afterContent)
        #[arg(long)]
        smart: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Line-numbered unified diff
    Diff {
        /// PR number
        number: u64,
        #[arg(short, long)]
        repo: String,
        /// Filter to specific files (substring match, repeatable)
        #[arg(short, long)]
        file: Vec<String>,
        /// Only show diffs for files with meaningful changes (auto-skips mechanical)
        #[arg(long)]
        smart_files: bool,
        /// Include lock files, generated files, and other noise (excluded by default)
        #[arg(long)]
        all: bool,
        /// Only show the stat table (no diff content)
        #[arg(long)]
        stat: bool,
        /// Output JSON with commentable lines map
        #[arg(long)]
        json: bool,
    },
    /// Read a file at the PR branch state
    File {
        /// PR number
        number: u64,
        #[arg(short, long)]
        repo: String,
        /// File path within the repo
        #[arg(short, long)]
        path: String,
    },
    /// Post batch review comments from a JSON file
    Review {
        /// PR number
        number: u64,
        #[arg(short, long)]
        repo: String,
        /// Path to JSON file with comments array
        #[arg(short, long)]
        comments_file: String,
    },
    /// Text search across PR files (or full repo at PR branch)
    Grep {
        /// PR number
        number: u64,
        #[arg(short, long)]
        repo: String,
        /// Search pattern (text)
        #[arg(short, long)]
        pattern: String,
        /// Filter to specific files (substring match, repeatable)
        #[arg(short, long)]
        file: Vec<String>,
        /// Search the entire repo via GitHub Code Search + PR changed files
        #[arg(long)]
        repo_wide: bool,
        /// Optional path prefix to narrow --repo-wide results (e.g. "src/")
        #[arg(long)]
        path: Option<String>,
        /// Search base branch instead of head
        #[arg(long)]
        base: bool,
        /// Case-sensitive search
        #[arg(long)]
        case_sensitive: bool,
        /// Lines of context around matches (like grep -C)
        #[arg(short = 'C', long, default_value = "0")]
        context: usize,
        /// Include lock/generated/minified files
        #[arg(long)]
        all: bool,
    },
    /// AST structural search across PR files (or full repo via Code Search)
    AstGrep {
        /// PR number
        number: u64,
        #[arg(short, long)]
        repo: String,
        /// AST pattern (e.g. "console.log($$$)")
        #[arg(short, long)]
        pattern: String,
        /// Filter to specific files (substring match, repeatable)
        #[arg(short, long)]
        file: Vec<String>,
        /// Search the entire repo via GitHub Code Search + PR changed files
        #[arg(long)]
        repo_wide: bool,
        /// Optional path prefix to narrow --repo-wide results (e.g. "src/")
        #[arg(long)]
        path: Option<String>,
        /// Search base branch instead of head
        #[arg(long)]
        base: bool,
        /// Language override (auto-detected from extension by default)
        #[arg(short, long)]
        lang: Option<String>,
        /// Include lock/generated/minified files
        #[arg(long)]
        all: bool,
    },
    /// Post a suggestion comment (GitHub suggestion block)
    Suggest {
        /// PR number
        number: u64,
        #[arg(short, long)]
        repo: String,
        /// File path
        #[arg(short, long)]
        file: String,
        /// Start line
        #[arg(long)]
        line_start: u64,
        /// End line (same as start for single-line)
        #[arg(long)]
        line_end: u64,
        /// Replacement code
        #[arg(long)]
        replacement: String,
    },
}
