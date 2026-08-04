use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "bkmsa",
    version,
    about = "Analyze spark profiler reports from a terminal",
    after_help = "Exit codes:\n  0 success\n  2 arguments or configuration\n  3 report read/download\n  4 protobuf decode\n  5 AI provider\n  6 analysis/output"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// List the deterministic report tools exposed by the analysis core.
    Tools(OutputArgs),
    /// Show the fields and capabilities present in a report.
    Inventory(ReportArgs),
    /// Parse a report and print its summary.
    Inspect(ReportArgs),
    /// Run one deterministic report tool.
    Tool(ToolArgs),
    /// Run the evidence-driven AI analysis agent.
    Analyze(AnalyzeArgs),
}

#[derive(Clone, Args)]
pub struct OutputArgs {
    /// Output representation.
    #[arg(long, value_enum, default_value_t = OutputFormat::Terminal)]
    pub format: OutputFormat,

    /// Write output to this file instead of stdout.
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

#[derive(Clone, Args)]
pub struct ReportArgs {
    /// Local file, '-' for stdin, spark viewer URL, content URL, or report key.
    pub source: String,

    /// Treat the input as UTF-8 text instead of spark protobuf (required for text on stdin).
    #[arg(long)]
    pub text: bool,

    #[command(flatten)]
    pub output: OutputArgs,
}

#[derive(Clone, Args)]
pub struct ToolArgs {
    /// Local file, '-' for stdin, spark viewer URL, content URL, or report key.
    pub source: String,

    /// Treat the input as UTF-8 text instead of spark protobuf (required for text on stdin).
    #[arg(long)]
    pub text: bool,

    /// Tool name, for example overview, hot_paths, or memory_gc.
    pub tool: String,

    /// Complete JSON object passed to the tool.
    #[arg(long, conflicts_with = "arg")]
    pub args: Option<String>,

    /// Tool argument in KEY=VALUE form. VALUE accepts JSON scalars/arrays/objects.
    #[arg(long = "arg", value_name = "KEY=VALUE")]
    pub arg: Vec<String>,

    /// Category convenience argument used by tools such as hot_paths.
    #[arg(long, conflicts_with = "args")]
    pub category: Option<String>,

    /// Limit convenience argument used by listing tools.
    #[arg(long, conflicts_with = "args")]
    pub limit: Option<usize>,

    /// Field path convenience argument used by raw_field.
    #[arg(long, conflicts_with = "args")]
    pub path: Option<String>,

    #[command(flatten)]
    pub output: OutputArgs,
}

#[derive(Clone, Args)]
pub struct AnalyzeArgs {
    /// Local file, '-' for stdin, spark viewer URL, content URL, or report key.
    pub source: String,

    /// Treat the input as UTF-8 text instead of spark protobuf (required for text on stdin).
    #[arg(long)]
    pub text: bool,

    /// TOML configuration file (defaults to the platform config directory).
    #[arg(long, env = "BKMSA_CONFIG")]
    pub config: Option<PathBuf>,

    #[command(flatten)]
    pub output: OutputArgs,

    /// OpenAI-compatible API key (prefer BKMSA_API_KEY to avoid shell history).
    #[arg(long, env = "BKMSA_API_KEY", hide_env_values = true)]
    pub api_key: Option<String>,

    /// OpenAI-compatible API base URL.
    #[arg(long, env = "BKMSA_BASE_URL")]
    pub base_url: Option<String>,

    /// Model name.
    #[arg(long, env = "BKMSA_MODEL")]
    pub model: Option<String>,

    /// Sampling temperature.
    #[arg(long, env = "BKMSA_TEMPERATURE")]
    pub temperature: Option<f32>,

    /// Maximum tool-agent rounds.
    #[arg(long, default_value_t = 12, value_parser = parse_max_rounds)]
    pub max_rounds: usize,
}

fn parse_max_rounds(value: &str) -> Result<usize, String> {
    let rounds = value
        .parse::<usize>()
        .map_err(|_| "max rounds must be an integer between 1 and 64".to_owned())?;
    if (1..=64).contains(&rounds) {
        Ok(rounds)
    } else {
        Err("max rounds must be between 1 and 64".to_owned())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Terminal,
    Json,
    Markdown,
}
