use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

use crate::commands;

#[derive(Debug, Parser)]
#[command(
    name = "lilaccaps",
    version,
    about = "Subtitle generation and burn-in CLI"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Doctor(DoctorArgs),
    Install(InstallArgs),
    Update(UpdateArgs),
    Status(StatusArgs),
    Uninstall(UninstallArgs),
    Transcribe(TranscribeArgs),
    Burnin(BurninArgs),
}

#[derive(Debug, Clone, Args)]
pub struct DoctorArgs {
    #[arg(long)]
    pub config_path: Option<PathBuf>,
    #[arg(long)]
    pub fix: bool,
}

#[derive(Debug, Clone, Args)]
pub struct InstallArgs {
    #[arg(long)]
    pub config_path: Option<PathBuf>,
    #[arg(long)]
    pub fix: bool,
}

#[derive(Debug, Clone, Args)]
pub struct UpdateArgs {
    #[arg(long)]
    pub config_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct StatusArgs {
    #[arg(long)]
    pub config_path: Option<PathBuf>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
pub struct UninstallArgs {
    #[arg(long)]
    pub config_path: Option<PathBuf>,
    #[arg(long)]
    pub yes: bool,
}

#[derive(Debug, Clone, Args)]
pub struct TranscribeArgs {
    pub input: PathBuf,
    #[arg(long)]
    pub config_path: Option<PathBuf>,
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct BurninArgs {
    pub video: PathBuf,
    #[arg(long)]
    pub config_path: Option<PathBuf>,
    #[arg(long)]
    pub subs: PathBuf,
    #[arg(long)]
    pub output: Option<PathBuf>,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Doctor(args) => commands::doctor::run(args),
        Command::Install(args) => commands::install::run(args),
        Command::Update(args) => commands::update::run(args),
        Command::Status(args) => commands::status::run(args),
        Command::Uninstall(args) => commands::uninstall::run(args),
        Command::Transcribe(args) => commands::transcribe::run(args),
        Command::Burnin(args) => commands::burnin::run(args),
    }
}
