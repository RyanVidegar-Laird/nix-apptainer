#![forbid(unsafe_code)]

use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod checks;
mod commands;
mod config;
mod container;
mod digest;
mod overlay;
mod paths;
mod sif;
mod state;
mod util;

#[derive(Parser)]
#[command(
    name = "nix-apptainer",
    about = "Manage nix-apptainer containers",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Guided first-time setup
    Init {
        /// Path to a local SIF file, a URL, or "github" (default)
        #[arg(long)]
        sif: Option<String>,
        /// Overlay size in MB (default: 51200)
        #[arg(long)]
        overlay_size: Option<u64>,
        /// Directory to store all data (overrides XDG paths)
        #[arg(long)]
        data_dir: Option<PathBuf>,
        /// Skip interactive prompts, accept defaults
        #[arg(short, long)]
        yes: bool,
    },
    /// Launch an interactive shell in the container
    Enter {
        /// Enable NVIDIA GPU passthrough
        #[arg(long)]
        nv: bool,
        /// Enable AMD ROCm GPU passthrough
        #[arg(long)]
        rocm: bool,
        /// Bind-mount a host path (SRC:DST)
        #[arg(long, short = 'B')]
        bind: Vec<String>,
        /// Extra arguments passed through to apptainer
        #[arg(last = true)]
        passthrough: Vec<String>,
    },
    /// Run a command in the container
    Exec {
        /// Enable NVIDIA GPU passthrough
        #[arg(long)]
        nv: bool,
        /// Enable AMD ROCm GPU passthrough
        #[arg(long)]
        rocm: bool,
        /// Bind-mount a host path (SRC:DST)
        #[arg(long, short = 'B')]
        bind: Vec<String>,
        /// Extra arguments passed through to apptainer
        #[arg(long, allow_hyphen_values = true, num_args = 0..)]
        passthrough: Vec<String>,
        /// Command and arguments to run
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    /// Check for and fetch a new base SIF image
    Update {
        /// Only check if an update is available (don't download)
        #[arg(long)]
        check: bool,
        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },
    /// Show current setup state
    Status,
    /// Remove data and configuration
    Clean {
        /// Remove everything (config, SIF, overlay, cache)
        #[arg(long)]
        all: bool,
        /// Remove only the download cache
        #[arg(long)]
        cache: bool,
        /// Remove only the overlay image
        #[arg(long)]
        overlay: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init {
            sif,
            overlay_size,
            data_dir,
            yes,
        } => commands::init::run(commands::init::InitFlags {
            sif,
            overlay_size,
            data_dir,
            yes,
        }),
        Commands::Enter {
            nv,
            rocm,
            bind,
            passthrough,
        } => commands::enter::run(commands::enter::EnterFlags {
            nv,
            rocm,
            bind,
            passthrough,
        }),
        Commands::Exec {
            nv,
            rocm,
            bind,
            passthrough,
            command,
        } => commands::exec::run(commands::exec::ExecFlags {
            nv,
            rocm,
            bind,
            passthrough,
            command,
        }),
        Commands::Update { check, yes } => {
            commands::update::run(commands::update::UpdateFlags { check, yes })
        }
        Commands::Status => commands::status::run(),
        Commands::Clean {
            all,
            cache,
            overlay,
        } => commands::clean::run(commands::clean::CleanFlags {
            all,
            cache,
            overlay,
        }),
    }
}
