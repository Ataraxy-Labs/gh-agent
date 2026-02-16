mod cli;
mod commands;
mod diff;
mod format;
mod github;
mod search;
mod sem;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands, PrCommands};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = github::Client::new()?;

    match cli.command {
        Commands::Pr { command } => match command {
            PrCommands::View {
                number,
                repo,
                sem,
                smart,
                json,
            } => {
                commands::pr_view(&client, &repo, number, sem, smart, json).await?;
            }
            PrCommands::Diff {
                number,
                repo,
                file,
                smart_files,
                all,
                stat,
                json,
            } => {
                commands::pr_diff(&client, &repo, number, &file, smart_files, all, stat, json).await?;
            }
            PrCommands::File { number, repo, path } => {
                commands::pr_file(&client, &repo, number, &path).await?;
            }
            PrCommands::Review {
                number,
                repo,
                comments_file,
            } => {
                commands::pr_review(&client, &repo, number, &comments_file).await?;
            }
            PrCommands::Grep {
                number,
                repo,
                pattern,
                file,
                repo_wide,
                path,
                base,
                case_sensitive,
                context,
                all,
            } => {
                commands::pr_grep(
                    &client, &repo, number, &pattern, &file,
                    repo_wide, path.as_deref(), base, case_sensitive, context, all,
                ).await?;
            }
            PrCommands::AstGrep {
                number,
                repo,
                pattern,
                file,
                repo_wide,
                path,
                base,
                lang,
                all,
            } => {
                commands::pr_ast_grep(
                    &client, &repo, number, &pattern, &file,
                    repo_wide, path.as_deref(), base, lang.as_deref(), all,
                ).await?;
            }
            PrCommands::Suggest {
                number,
                repo,
                file,
                line_start,
                line_end,
                replacement,
            } => {
                commands::pr_suggest(
                    &client,
                    &repo,
                    number,
                    &file,
                    line_start,
                    line_end,
                    &replacement,
                )
                .await?;
            }
        },
    }

    Ok(())
}
