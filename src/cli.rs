use std::ffi::OsString;
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
    /// Manage saved text and image watermarks
    WatermarkPreset(WatermarkPresetArgs),
    /// Transcribe, review, translate, and render a resumable caption project
    Workflow(WorkflowArgs),
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
    #[arg(long, help = "Skip managed system dependency updates")]
    pub skip_dependencies: bool,
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
    #[arg(long, value_name = "ENGINE")]
    pub engine: Option<String>,
    #[arg(long, value_name = "MODEL")]
    pub model: Option<String>,
    #[arg(long, value_name = "MODEL", num_args = 0..=1, default_missing_value = "")]
    pub cleanup: Option<String>,
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
    pub config_path: Option<PathBuf>,
    #[arg(long, conflicts_with_all = ["text", "image", "position", "opacity", "size", "margin", "colour", "font", "outline_colour", "outline_width"])]
    pub preset: Option<String>,
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

#[derive(Debug, Clone, Args)]
pub struct WatermarkPresetArgs {
    #[command(subcommand)]
    pub command: WatermarkPresetCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum WatermarkPresetCommand {
    /// Save a new named watermark, including copies of image and font assets
    Save(WatermarkPresetSaveArgs),
    List(WatermarkPresetListArgs),
    Show(WatermarkPresetShowArgs),
    /// Remove one saved preset and its owned assets
    Remove(WatermarkPresetRemoveArgs),
}

#[derive(Debug, Clone, Args)]
pub struct WatermarkPresetSaveArgs {
    pub name: String,
    #[arg(long)]
    pub config_path: Option<PathBuf>,
    #[arg(long, required_unless_present = "image", conflicts_with = "image")]
    pub text: Option<String>,
    #[arg(long, required_unless_present = "text")]
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
pub struct WatermarkPresetListArgs {
    #[arg(long)]
    pub config_path: Option<PathBuf>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
pub struct WatermarkPresetShowArgs {
    pub name: String,
    #[arg(long)]
    pub config_path: Option<PathBuf>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
pub struct WatermarkPresetRemoveArgs {
    pub name: String,
    #[arg(long)]
    pub config_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct WorkflowArgs {
    #[command(subcommand)]
    pub command: WorkflowCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum WorkflowCommand {
    /// Prepare reviewed subtitles and stop for review; never renders implicitly
    Start(WorkflowStartArgs),
    /// Continue missing stages, or translate again after source captions were edited
    Resume(WorkflowProjectArgs),
    /// Show files, review issues, and the next action
    Status(WorkflowStatusArgs),
    /// Record review of the current captions and watermark
    Accept(WorkflowAcceptArgs),
    /// Render accepted captions and optional watermark, then verify the output
    Render(WorkflowRenderArgs),
}

#[derive(Debug, Clone, Args)]
pub struct WorkflowStartArgs {
    pub video: PathBuf,
    #[arg(long, help = "New project directory (default: <video-stem>.lilaccaps)")]
    pub project: Option<PathBuf>,
    #[arg(long)]
    pub config_path: Option<PathBuf>,
    #[arg(
        long,
        help = "Use an existing source SRT instead of transcribing again"
    )]
    pub subs: Option<PathBuf>,
    #[arg(
        long = "to",
        help = "Optional target language; only this translation is selected for rendering"
    )]
    pub to: Option<String>,
    #[arg(long = "lang", visible_alias = "language")]
    pub lang: Option<String>,
    #[arg(long)]
    pub engine: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
    #[arg(
        long,
        help = "Text file with names, terminology and context for the caption agent"
    )]
    pub context_file: Option<PathBuf>,
    #[arg(long, help = "Saved watermark name to copy into this project")]
    pub watermark: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct WorkflowProjectArgs {
    pub project: PathBuf,
    #[arg(
        long,
        help = "Run a new agent pass on the current source captions, keeping the previous revision"
    )]
    pub review_source: bool,
    #[arg(
        long,
        help = "Create a new translation revision even when the source is unchanged"
    )]
    pub retranslate: bool,
}

#[derive(Debug, Clone, Args)]
pub struct WorkflowStatusArgs {
    pub project: PathBuf,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
pub struct WorkflowAcceptArgs {
    pub project: PathBuf,
    #[arg(
        long,
        help = "Record what you checked, including any remaining uncertainty"
    )]
    pub note: String,
}

#[derive(Debug, Clone, Args)]
pub struct WorkflowRenderArgs {
    pub project: PathBuf,
    #[arg(long, help = "New output MP4 path (default: <project>/final.mp4)")]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub font: Option<String>,
    #[arg(
        long,
        help = "Caption size in native ASS units, or point size for the overlay renderer"
    )]
    pub size: Option<u32>,
}

pub fn run() -> Result<()> {
    let args = std::env::args_os().collect::<Vec<_>>();
    if is_root_version_request(&args) {
        return commands::version::run();
    }
    let cli = Cli::parse_from(args);

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
        Command::WatermarkPreset(args) => commands::watermark_preset::run(args),
        Command::Workflow(args) => commands::workflow::run(args),
    }
}

fn is_root_version_request(args: &[OsString]) -> bool {
    matches!(args, [_, flag] if flag == "--version" || flag == "-V")
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::is_root_version_request;

    #[test]
    fn intercepts_only_exact_root_version_requests() {
        assert!(is_root_version_request(&[
            OsString::from("lilaccaps"),
            OsString::from("--version"),
        ]));
        assert!(is_root_version_request(&[
            OsString::from("lilaccaps"),
            OsString::from("-V"),
        ]));
        assert!(!is_root_version_request(&[
            OsString::from("lilaccaps"),
            OsString::from("status"),
        ]));
        assert!(!is_root_version_request(&[
            OsString::from("lilaccaps"),
            OsString::from("--version"),
            OsString::from("status"),
        ]));
    }
}
