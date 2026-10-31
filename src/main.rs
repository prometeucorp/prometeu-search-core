use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use prometeu_search::{Direction, Engine, GraphMode, GraphOptions, mcp};
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
    /// Find candidate callers using static resolution evidence.
    Callers(Query),
    /// Find candidate callees using static resolution evidence.
    Callees(Query),
    /// Assemble relevant source, relationships, and impact in one bounded response.
    Explore(GraphQuery),
    /// Find directly and indirectly affected symbols.
    Impact(GraphQuery),
    /// Find a candidate directed path between two symbols.
    Path {
        #[command(flatten)]
        query: GraphQuery,
        target: String,
    },
    /// Find types that extend or implement a type.
    Implementations(GraphQuery),
    /// Find candidate test files affected through import dependencies.
    Affected(GraphQuery),
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

#[derive(Args)]
struct GraphQuery {
    #[command(flatten)]
    search: Query,
    /// Restrict symbol selection to this worktree-relative file.
    #[arg(long)]
    path: Option<String>,
    /// Select the symbol containing this line; requires --path.
    #[arg(long, requires = "path", value_parser = clap::value_parser!(u32).range(1..))]
    line: Option<u32>,
    /// Maximum graph traversal depth.
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(1..=8))]
    depth: u8,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut engine = Engine::open(&cli.root, cli.database.as_deref())?;
    match cli.command {
        Command::Index => output(&engine.sync()?),
        Command::Search(query) => {
            engine.refresh()?;
            output(&engine.search(&query.query, query.limit.into(), query.max_bytes as usize)?)
        }
        Command::Callers(query) => relations(&mut engine, query, Direction::Callers),
        Command::Callees(query) => relations(&mut engine, query, Direction::Callees),
        Command::Explore(query) => graph(&mut engine, query, None, None),
        Command::Impact(query) => graph(&mut engine, query, Some(GraphMode::Impact), None),
        Command::Path { query, target } => {
            graph(&mut engine, query, Some(GraphMode::Path), Some(target))
        }
        Command::Implementations(query) => {
            graph(&mut engine, query, Some(GraphMode::Implementations), None)
        }
        Command::Affected(query) => graph(&mut engine, query, Some(GraphMode::Affected), None),
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
    engine.refresh()?;
    output(&engine.related(
        &query.query,
        direction,
        query.limit.into(),
        query.max_bytes as usize,
    )?)
}

fn graph(
    engine: &mut Engine,
    query: GraphQuery,
    mode: Option<GraphMode>,
    target: Option<String>,
) -> Result<()> {
    let options = GraphOptions {
        path: query.path,
        line: query.line.map(|line| line as usize),
        target,
        depth: query.depth.into(),
        limit: query.search.limit.into(),
        max_bytes: query.search.max_bytes as usize,
    };
    engine.refresh()?;
    match mode {
        Some(mode) => output(&engine.analyze(&query.search.query, mode, &options)?),
        None => output(&engine.explore(&query.search.query, &options)?),
    }
}

fn output(value: &impl Serialize) -> Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, value)?;
    writeln!(stdout)?;
    Ok(())
}
