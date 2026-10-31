mod mcp;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use prometeu_search::{Direction, Engine};
use serde::Serialize;
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Directory to index. Each canonical root owns a separate database.
    #[arg(long, global = true, default_value = ".")]
    root: PathBuf,
    /// Override the private cache database location.
    #[arg(long, global = true)]
    database: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create or reconcile the index with files currently on disk.
    Index,
    /// Find symbols and source with a bounded JSON response.
    Search(Query),
    /// Find candidate callers of a symbol by static name matching.
    Callers(Query),
    /// Find candidate callees of a symbol by static name matching.
    Callees(Query),
    /// Show the last committed index state without changing it.
    Status,
    /// Serve a worktree-scoped MCP server over newline-delimited stdio.
    Serve {
        /// Disable background watching; queries still reconcile the index.
        #[arg(long)]
        no_watch: bool,
    },
}

#[derive(Args)]
struct Query {
    query: String,
    /// Maximum number of matches.
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u16).range(1..=50))]
    limit: u16,
    /// Maximum UTF-8 bytes in the JSON result, excluding transport framing.
    #[arg(long, default_value_t = 12000, value_parser = clap::value_parser!(u32).range(1024..=65536))]
    max_bytes: u32,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut engine = Engine::open(&cli.root, cli.database.as_deref())?;
    match cli.command {
        Command::Index => output(&engine.sync()?),
        Command::Search(query) => {
            engine.sync()?;
            output(&engine.search(&query.query, query.limit.into(), query.max_bytes as usize)?)
        }
        Command::Callers(query) => relations(&mut engine, query, Direction::Callers),
        Command::Callees(query) => relations(&mut engine, query, Direction::Callees),
        Command::Status => output(&engine.status()?),
        Command::Serve { no_watch } => {
            let _watcher = if no_watch {
                None
            } else {
                match engine.watch() {
                    Ok(watcher) => Some(watcher),
                    Err(error) => {
                        eprintln!(
                            "Background watching unavailable: {error:#}. Queries still reconcile files."
                        );
                        None
                    }
                }
            };
            mcp::serve(&mut engine, io::stdin().lock(), io::stdout().lock())
        }
    }
}

fn relations(engine: &mut Engine, query: Query, direction: Direction) -> Result<()> {
    engine.sync()?;
    output(&engine.related(
        &query.query,
        direction,
        query.limit.into(),
        query.max_bytes as usize,
    )?)
}

fn output(value: &impl Serialize) -> Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, value)?;
    writeln!(stdout)?;
    Ok(())
}
