use anyhow::Result;

use crate::cli::{WorkflowArgs, WorkflowCommand};
use crate::workflow::{self, WorkflowStatus};

pub fn run(args: WorkflowArgs) -> Result<()> {
    let status = match args.command {
        WorkflowCommand::Start(args) => workflow::start(args)?,
        WorkflowCommand::Resume(args) => {
            workflow::resume(&args.project, args.review_source, args.retranslate)?
        }
        WorkflowCommand::Status(args) => {
            let status = workflow::status(&args.project)?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&status)?);
                return Ok(());
            }
            status
        }
        WorkflowCommand::Accept(args) => workflow::accept(&args.project, &args.note)?,
        WorkflowCommand::Render(args) => workflow::render(args)?,
    };
    print_status(status);
    Ok(())
}

fn print_status(status: WorkflowStatus) {
    println!("command = workflow");
    println!("project = {}", status.project.display());
    println!("state = {}", status.state);
    if let Some(path) = status.source_subtitles {
        println!("source_subtitles = {}", path.display());
    }
    if let Some(path) = status.selected_subtitles {
        println!("selected_subtitles = {}", path.display());
    }
    println!("review = {}", status.review_file.display());
    println!("review_issues = {}", status.issues.len());
    if let Some(path) = status.last_output {
        println!("last_output = {}", path.display());
    }
    println!("next_action = {}", status.next_action);
}
