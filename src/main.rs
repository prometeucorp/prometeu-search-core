mod mcp;

use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};
use prometeu_search::context::{BuildContextOptions, TaskInput};
use prometeu_search::{
    DeadCodeOptions, Direction, Engine, GraphMode, GraphOperation, GraphOptions, GraphQueryOptions,
};
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
    /// Query canonical graph traversal, call sites, dependencies, and symbol context.
    Graph(GraphRequest),
    /// Browse the indexed graph in a local web interface.
    Ui {
        #[arg(long, default_value_t = 3333)]
        port: u16,
        /// Disable saved-trail writes.
        #[arg(long)]
        read_only: bool,
    },
    /// Create or reconcile the index with files currently on disk.
    Index,
    /// Find symbols and source with a bounded JSON response.
    Search(SearchQuery),
    /// Find candidate callers using static resolution evidence.
    Callers(Query),
    /// Find candidate callees using static resolution evidence.
    Callees(Query),
    /// Assemble relevant source, relationships, and impact in one bounded response.
    Explore(GraphQuery),
    /// Rank relevant symbols and assemble task context with source and relationships.
    Context(ContextQuery),
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
    /// List indexed files with language and symbol counts.
    Files {
        #[arg(default_value = "")]
        query: String,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        max_bytes: Option<usize>,
    },
    /// Read a search result's source and complete extraction metadata.
    Node {
        #[arg(value_parser = clap::value_parser!(i64).range(1..))]
        id: i64,
        #[arg(long)]
        max_bytes: Option<usize>,
    },
    /// Show a type's ancestry, subtypes, and member override matches.
    Hierarchy {
        #[arg(value_parser = clap::value_parser!(i64).range(1..))]
        id: i64,
        #[arg(long)]
        max_bytes: Option<usize>,
    },
    /// Count resolved edges, calls, children, and containment depth for a symbol ID.
    Metrics {
        #[arg(value_parser = clap::value_parser!(i64).range(1..))]
        id: i64,
        #[arg(long)]
        max_bytes: Option<usize>,
    },
    /// List groups of files that reach each other through imports.
    Cycles {
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        max_bytes: Option<usize>,
    },
    /// Report unreferenced symbols after conservative reachability exclusions.
    Deadcode {
        /// Comma-separated declaration kinds; empty uses callable and type kinds.
        #[arg(long, value_delimiter = ',')]
        kinds: Vec<String>,
        #[arg(long)]
        include_exported: bool,
        #[arg(long)]
        include_tests: bool,
        #[arg(long)]
        include_generated: bool,
        #[arg(long, default_value_t = 200)]
        limit: usize,
        #[arg(long)]
        max_bytes: Option<usize>,
    },
    /// Read inclusive lines from the committed file snapshot.
    File {
        path: String,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
        start_line: u32,
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        end_line: Option<u32>,
        #[arg(long)]
        max_bytes: Option<usize>,
    },
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
struct GraphRequest {
    #[arg(value_parser = graph_operation)]
    operation: GraphOperation,
    #[arg(long, value_parser = clap::value_parser!(i64).range(1..))]
    id: Option<i64>,
    #[arg(long, value_parser = clap::value_parser!(i64).range(1..))]
    target: Option<i64>,
    #[arg(long)]
    path: Option<String>,
    #[arg(long)]
    pattern: Option<String>,
    #[arg(long)]
    depth: Option<usize>,
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long)]
    max_edges: Option<usize>,
    #[arg(long, value_delimiter = ',')]
    edge_kinds: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    node_kinds: Vec<String>,
    #[arg(long, default_value = "outgoing", value_parser = graph_direction)]
    direction: prometeu_search::graph_query::Direction,
    #[arg(long)]
    exclude_start: bool,
    #[arg(long)]
    no_edges: bool,
    #[arg(long)]
    max_bytes: Option<usize>,
}

fn graph_operation(value: &str) -> std::result::Result<GraphOperation, String> {
    serde_json::from_value(serde_json::Value::String(value.to_owned())).map_err(|e| e.to_string())
}

fn graph_direction(
    value: &str,
) -> std::result::Result<prometeu_search::graph_query::Direction, String> {
    serde_json::from_value(serde_json::Value::String(value.to_owned())).map_err(|e| e.to_string())
}

#[derive(Args)]
struct Query {
    query: String,
    /// Maximum number of matches.
    #[arg(long)]
    limit: Option<usize>,
    /// Maximum UTF-8 bytes in the JSON result, excluding transport framing.
    #[arg(long)]
    max_bytes: Option<usize>,
}

#[derive(Args)]
struct SearchQuery {
    #[command(flatten)]
    search: Query,
    /// Skip results after exact-name supplementation and final ranking.
    #[arg(long, default_value_t = 0)]
    offset: usize,
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
    #[arg(long)]
    depth: Option<usize>,
}

#[derive(Clone, Copy, ValueEnum)]
enum ContextFormat {
    Json,
    Markdown,
}

#[derive(Args)]
struct ContextQuery {
    query: String,
    #[arg(long, default_value_t = 20)]
    max_nodes: usize,
    #[arg(long, default_value_t = 5)]
    max_code_blocks: usize,
    #[arg(long, default_value_t = 1500)]
    max_code_block_size: usize,
    #[arg(long)]
    no_code: bool,
    #[arg(long, default_value_t = 3)]
    search_limit: usize,
    #[arg(long, default_value_t = 1)]
    depth: usize,
    #[arg(long, default_value_t = 0.3)]
    min_score: f64,
    #[arg(long, value_enum, default_value = "json")]
    format: ContextFormat,
    #[arg(long)]
    max_bytes: Option<usize>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut engine = Engine::open(&cli.root, cli.database.as_deref())?;
    match cli.command {
        Command::Graph(query) => {
            engine.refresh()?;
            let mut options = GraphQueryOptions::for_operation(query.operation);
            options.id = query.id;
            options.target = query.target;
            options.path = query.path;
            options.pattern = query.pattern;
            if let Some(depth) = query.depth {
                options.traversal.max_depth = depth;
            }
            if let Some(limit) = query.limit {
                options.traversal.limit = limit;
            }
            if let Some(max_edges) = query.max_edges {
                options.max_edges = max_edges;
            }
            options.traversal.edge_kinds = query.edge_kinds;
            options.traversal.node_kinds = query.node_kinds;
            options.traversal.direction = query.direction;
            options.traversal.include_start = !query.exclude_start;
            options.include_edges = !query.no_edges;
            options.max_bytes = query.max_bytes.unwrap_or(usize::MAX);
            output(&engine.graph_query(&options)?)
        }
        Command::Ui { port, read_only } => {
            prometeu_search::viewer::serve_with_options(&mut engine, port, read_only)
        }
        Command::Index => output(&engine.sync()?),
        Command::Search(query) => {
            engine.refresh()?;
            output(&engine.search_with_options(
                &query.search.query,
                &prometeu_search::search::SearchOptions {
                    limit: query.search.limit.unwrap_or(10),
                    offset: query.offset,
                    max_bytes: query.search.max_bytes,
                },
            )?)
        }
        Command::Callers(query) => relations(&mut engine, query, Direction::Callers),
        Command::Callees(query) => relations(&mut engine, query, Direction::Callees),
        Command::Explore(query) => graph(&mut engine, query, None, None),
        Command::Context(query) => {
            engine.refresh()?;
            let result = engine.context(
                &TaskInput::Query(query.query),
                &BuildContextOptions {
                    max_nodes: query.max_nodes,
                    max_code_blocks: query.max_code_blocks,
                    max_code_block_size: query.max_code_block_size,
                    include_code: !query.no_code,
                    search_limit: query.search_limit,
                    traversal_depth: query.depth,
                    min_score: query.min_score,
                },
                query.max_bytes.unwrap_or(usize::MAX),
            )?;
            match query.format {
                ContextFormat::Json => output(&result),
                ContextFormat::Markdown => {
                    io::stdout()
                        .lock()
                        .write_all(result.markdown().as_bytes())?;
                    Ok(())
                }
            }
        }
        Command::Impact(query) => graph(&mut engine, query, Some(GraphMode::Impact), None),
        Command::Path { query, target } => {
            graph(&mut engine, query, Some(GraphMode::Path), Some(target))
        }
        Command::Implementations(query) => {
            graph(&mut engine, query, Some(GraphMode::Implementations), None)
        }
        Command::Affected(query) => graph(&mut engine, query, Some(GraphMode::Affected), None),
        Command::Files {
            query,
            limit,
            max_bytes,
        } => {
            engine.refresh()?;
            output(&engine.files(
                &query,
                limit.unwrap_or(usize::MAX),
                max_bytes.unwrap_or(usize::MAX),
            )?)
        }
        Command::Node { id, max_bytes } => {
            engine.refresh()?;
            output(&engine.node(id, max_bytes.unwrap_or(usize::MAX))?)
        }
        Command::Metrics { id, max_bytes } => {
            engine.refresh()?;
            output(&engine.metrics(id, max_bytes.unwrap_or(usize::MAX))?)
        }
        Command::Hierarchy { id, max_bytes } => {
            engine.refresh()?;
            output(&engine.hierarchy(id, max_bytes.unwrap_or(usize::MAX))?)
        }
        Command::Cycles { limit, max_bytes } => {
            engine.refresh()?;
            output(&engine.cycles(limit.unwrap_or(usize::MAX), max_bytes.unwrap_or(usize::MAX))?)
        }
        Command::Deadcode {
            kinds,
            include_exported,
            include_tests,
            include_generated,
            limit,
            max_bytes,
        } => {
            engine.refresh()?;
            output(&engine.dead_code(&DeadCodeOptions {
                kinds,
                include_exported,
                include_tests,
                include_generated,
                limit,
                max_bytes: max_bytes.unwrap_or(usize::MAX),
            })?)
        }
        Command::File {
            path,
            start_line,
            end_line,
            max_bytes,
        } => {
            engine.refresh()?;
            output(&engine.file(
                &path,
                start_line as usize,
                end_line.map(|line| line as usize),
                max_bytes.unwrap_or(usize::MAX),
            )?)
        }
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
        query.limit.unwrap_or(20),
        query.max_bytes.unwrap_or(usize::MAX),
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
        depth: query.depth.unwrap_or(if mode == Some(GraphMode::Impact) {
            3
        } else {
            2
        }),
        limit: query
            .search
            .limit
            .unwrap_or(if mode == Some(GraphMode::Impact) {
                10_000
            } else {
                200
            }),
        max_bytes: query.search.max_bytes.unwrap_or(usize::MAX),
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
