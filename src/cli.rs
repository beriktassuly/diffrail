use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    name = "agent-change-control",
    version,
    about = "Enforce scoped repository changes with a policy stored in Git"
)]
pub struct Cli {
    /// Repository path. Defaults to the current directory.
    #[arg(long, global = true, value_name = "PATH")]
    pub repo: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub fn output_format(&self) -> OutputFormat {
        match &self.command {
            Command::Check { format, .. } | Command::Validate { format, .. } => *format,
            Command::Init { .. } => OutputFormat::Human,
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a safe starter policy in the repository root.
    Init {
        /// Repository-relative policy path.
        #[arg(long, default_value = ".agent-change-control.yml", value_name = "PATH")]
        config: PathBuf,
    },

    /// Validate the policy currently present in the working tree.
    Validate {
        /// Repository-relative policy path.
        #[arg(long, default_value = ".agent-change-control.yml", value_name = "PATH")]
        config: PathBuf,

        /// Output format.
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,
    },

    /// Check repository changes against a task boundary.
    Check {
        /// Task ID defined in the trusted policy.
        #[arg(long, value_name = "ID")]
        task: String,

        /// Trusted policy revision. Its merge-base with head starts the diff.
        #[arg(long, value_name = "REV")]
        base: Option<String>,

        /// Revision to compare with base. Requires --base when not HEAD.
        #[arg(long, default_value = "HEAD", value_name = "REV")]
        head: String,

        /// Repository-relative policy path.
        #[arg(long, default_value = ".agent-change-control.yml", value_name = "PATH")]
        config: PathBuf,

        /// Output format.
        #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
        format: OutputFormat,

        /// Suppress successful human-readable output.
        #[arg(long)]
        quiet: bool,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    #[default]
    Human,
    Json,
    Github,
}

impl OutputFormat {
    pub fn is_json(self) -> bool {
        self == Self::Json
    }
}
