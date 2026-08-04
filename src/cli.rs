use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::engine::ArgValueCandidates;
use clap_complete::CompletionCandidate;

use crate::client;
use crate::commands;

pub fn command() -> clap::Command {
    Cli::command()
}

pub fn run() {
    match Cli::parse().command {
        Command::Login { url, user } => commands::login::run(url, user),
        Command::Clean { all } => commands::clean::run(all),
        Command::Problems => commands::problems::run(),
        Command::New { problem, force } => commands::new::run(&problem, force),
        Command::View {
            problem,
            text,
            pdf,
            no_open,
        } => commands::view::run(&problem, text, pdf, no_open),
        Command::Run { file } => commands::run::run(&file),
        Command::Submit {
            file,
            problem,
            no_wait,
            no_check,
        } => commands::submit::run(&file, problem.as_deref(), no_wait, no_check),
        Command::Status {
            submission,
            problem,
        } => commands::status::run(submission, problem.as_deref()),
    }
}

fn complete_problem() -> Vec<CompletionCandidate> {
    client::cached_problems()
        .into_iter()
        .map(|(name, title)| {
            CompletionCandidate::new(name).help(if title.is_empty() {
                None
            } else {
                Some(title.into())
            })
        })
        .collect()
}

fn existing_file(value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if !path.exists() {
        return Err(format!("file '{value}' does not exist"));
    }
    if path.is_dir() {
        return Err(format!("'{value}' is a directory"));
    }
    Ok(path)
}

#[derive(Parser)]
#[command(
    name = "kafae",
    about = "Submit coursework to Cafe Grader and get the verdict without leaving the terminal.",
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Authenticate and cache the 12h token.")]
    Login {
        #[arg(long, help = "Grader base url.")]
        url: Option<String>,
        #[arg(long, help = "Login name.")]
        user: Option<String>,
    },
    #[command(about = "Delete the cached problem list and downloaded statements.")]
    Clean {
        #[arg(
            long,
            help = "Also forget the login token, so you have to log in again."
        )]
        all: bool,
    },
    #[command(about = "List problems you can submit to.")]
    Problems,
    #[command(about = "Start a solution named after the problem, so submit needs no -p.")]
    New {
        #[arg(
            help = "Problem name, id, or glob (quote it: '01_Expr_*').",
            add = ArgValueCandidates::new(complete_problem)
        )]
        problem: String,
        #[arg(long, help = "Overwrite an existing file.")]
        force: bool,
    },
    #[command(about = "Open the problem statement: the PDF in your viewer, the description here.")]
    View {
        #[arg(help = "Problem name or id.", add = ArgValueCandidates::new(complete_problem))]
        problem: String,
        #[arg(long, help = "Description only, skip the PDF.")]
        text: bool,
        #[arg(long, help = "View the PDF in the terminal with tdf.")]
        pdf: bool,
        #[arg(long, help = "Fetch the PDF but don't open a viewer.")]
        no_open: bool,
    },
    #[command(
        about = "Compile and run locally; stdin/stdout pass through, so pipes and redirects work."
    )]
    Run {
        #[arg(value_parser = existing_file)]
        file: PathBuf,
    },
    #[command(about = "Submit a file and block for the verdict; exit 0 only on full marks.")]
    Submit {
        #[arg(value_parser = existing_file)]
        file: PathBuf,
        #[arg(
            short,
            long,
            help = "Problem name or id.",
            add = ArgValueCandidates::new(complete_problem)
        )]
        problem: Option<String>,
        #[arg(long, help = "Don't poll for the verdict.")]
        no_wait: bool,
        #[arg(long, help = "Skip the local compile check.")]
        no_check: bool,
    },
    #[command(about = "Verdict of a submission (default: latest for -p).")]
    Status {
        #[arg(help = "Submission id.")]
        submission: Option<i64>,
        #[arg(
            short,
            long,
            help = "Problem name or id.",
            add = ArgValueCandidates::new(complete_problem)
        )]
        problem: Option<String>,
    },
}
