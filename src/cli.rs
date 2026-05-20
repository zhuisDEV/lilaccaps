use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

use crate::commands;
use crate::watermark::WatermarkPosition;

#[derive(Debug, Parser)]
#[command(
    name = "lilaccaps",
    version,
    about = "Subtitle generation, burn-in, and watermark CLI"
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
    Translate(TranslateArgs),
    Burnin(BurninArgs),
    Watermark(WatermarkArgs),
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
    #[arg(long = "lang", visible_alias = "language")]
    pub lang: Option<String>,
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
    #[arg(long)]
    pub font: Option<String>,
    #[arg(long = "colour", visible_alias = "color")]
    pub colour: Option<String>,
    #[arg(long)]
    pub size: Option<u32>,
    #[arg(long, conflicts_with = "no_outline")]
    pub outline: bool,
    #[arg(long, conflicts_with = "outline")]
    pub no_outline: bool,
    #[arg(
        long = "outline-colour",
        visible_alias = "outline-color",
        conflicts_with = "no_outline"
    )]
    pub outline_colour: Option<String>,
    #[arg(long, conflicts_with = "no_outline")]
    pub outline_width: Option<u32>,
}

#[derive(Debug, Clone, Args)]
pub struct WatermarkArgs {
    pub video: PathBuf,
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub text: Option<String>,
    #[arg(long)]
    pub image: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = WatermarkPosition::BottomRight)]
    pub position: WatermarkPosition,
    #[arg(long, default_value_t = 0.4)]
    pub opacity: f32,
    #[arg(long, default_value_t = 0)]
    pub size: u32,
    #[arg(long, default_value_t = 24)]
    pub margin: u32,
    #[arg(long = "colour", visible_alias = "color", default_value = "white")]
    pub colour: String,
    #[arg(long)]
    pub font: Option<String>,
    #[arg(
        long = "outline-colour",
        visible_alias = "outline-color",
        default_value = "black"
    )]
    pub outline_colour: String,
    #[arg(long, default_value_t = 0)]
    pub outline_width: u32,
}

#[derive(Debug, Clone, Args)]
pub struct TranslateArgs {
    pub input: PathBuf,
    #[arg(long)]
    pub config_path: Option<PathBuf>,
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long = "to")]
    pub to: Vec<String>,
    #[arg(long)]
    pub append: Option<bool>,
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
        Command::Translate(args) => commands::translate::run(args),
        Command::Burnin(args) => commands::burnin::run(args),
        Command::Watermark(args) => commands::watermark::run(args),
    }
}
