use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::engine::ArgValueCandidates;
use clap_complete::CompletionCandidate;

use crate::client;
use crate::commands;
use crate::templates;

pub fn command() -> clap::Command {
    Cli::command()
}

pub fn run() {
    match Cli::parse().command {
        Command::Login { url, user } => commands::login::run(url, user),
        Command::Clean { all, problem } => commands::clean::run(all, problem.as_deref()),
        Command::Whoami => commands::whoami::run(),
        Command::Problems {
            pattern,
            solved,
            unsolved,
            untried,
            partial,
            tag,
            sort,
            reverse,
        } => commands::problems::run(
            commands::problems::Filter {
                pattern,
                solved,
                unsolved,
                untried,
                partial,
                tag,
            },
            sort,
            reverse,
        ),
        Command::New {
            problem,
            template,
            force,
        } => commands::new::run(&problem, &template, force),
        Command::Templates => commands::templates::run(),
        Command::View {
            problem,
            text,
            open,
            open_with,
            detach,
        } => commands::view::run(&problem, text, open, open_with.as_deref(), detach),
        Command::Run { file } => commands::run::run(&file),
        Command::Test { file, problem } => commands::test::run(&file, problem.as_deref()),
        Command::Submit {
            file,
            problem,
            no_wait,
            no_check,
        } => commands::submit::run(&file, problem.as_deref(), no_wait, no_check),
        Command::Open {
            problem,
            submission,
        } => commands::open::run(problem.as_deref(), submission),
        Command::Diff { file, problem } => commands::diff::run(&file, problem.as_deref()),
        Command::History { problem } => commands::history::run(&problem),
        Command::Get {
            submission,
            problem,
            output,
            force,
        } => commands::get::run(submission, problem.as_deref(), output.as_deref(), force),
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

// for new, a problem with a solution file in the cwd is already taken
fn complete_new_problem() -> Vec<CompletionCandidate> {
    let taken: std::collections::HashSet<String> = std::fs::read_dir(".")
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_file())
                .filter_map(|path| Some(path.file_stem()?.to_str()?.to_string()))
                .collect()
        })
        .unwrap_or_default();
    client::cached_problems()
        .into_iter()
        .filter(|(name, _)| !taken.contains(name))
        .map(|(name, title)| {
            CompletionCandidate::new(name).help(if title.is_empty() {
                None
            } else {
                Some(title.into())
            })
        })
        .collect()
}

fn complete_template() -> Vec<CompletionCandidate> {
    templates::names()
        .into_iter()
        .map(CompletionCandidate::new)
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
    #[command(about = "Delete the cached problem list, statements and testcases.")]
    Clean {
        #[arg(
            long,
            help = "Also forget the login token, so you have to log in again.",
            conflicts_with = "problem"
        )]
        all: bool,
        #[arg(
            short,
            long,
            help = "Only this problem's testcases and statement.",
            add = ArgValueCandidates::new(complete_problem)
        )]
        problem: Option<String>,
    },
    #[command(about = "Show who the cached token belongs to.")]
    Whoami,
    #[command(about = "List problems you can submit to.")]
    Problems {
        #[arg(help = "Name or title glob; a plain word matches anywhere.")]
        pattern: Option<String>,
        #[arg(long, group = "state", help = "Only ones you have full marks on.")]
        solved: bool,
        #[arg(long, group = "state", help = "Only ones short of full marks.")]
        unsolved: bool,
        #[arg(long, group = "state", help = "Only ones you have never submitted to.")]
        untried: bool,
        #[arg(
            long,
            group = "state",
            help = "Only ones you tried but have not solved."
        )]
        partial: bool,
        #[arg(short, long, help = "Only ones carrying this tag.")]
        tag: Option<String>,
        #[arg(
            long,
            value_enum,
            default_value = "name",
            help = "Order the list; problems the key says nothing about sort last."
        )]
        sort: commands::problems::Sort,
        #[arg(long, help = "Flip the order.")]
        reverse: bool,
    },
    #[command(about = "Start a solution named after the problem, so submit needs no -p.")]
    New {
        #[arg(
            help = "Problem name, id, or glob (quote it: '01_Expr_*').",
            add = ArgValueCandidates::new(complete_new_problem)
        )]
        problem: String,
        #[arg(
            short,
            long,
            default_value = "default",
            help = "Template name, see kafae templates.",
            add = ArgValueCandidates::new(complete_template)
        )]
        template: String,
        #[arg(long, help = "Overwrite an existing file.")]
        force: bool,
    },
    #[command(about = "List templates for new; yours live next to the builtins.")]
    Templates,
    #[command(
        about = "Show the problem statement; the PDF is fetched but only opened if you ask."
    )]
    View {
        #[arg(help = "Problem name or id.", add = ArgValueCandidates::new(complete_problem))]
        problem: String,
        #[arg(
            long,
            help = "Description only, skip the PDF.",
            conflicts_with_all = ["open", "open_with", "detach"]
        )]
        text: bool,
        #[arg(long, help = "Hand the PDF to your desktop viewer and carry on.")]
        open: bool,
        #[arg(
            long,
            value_name = "CMD",
            help = "Open the PDF with this command and wait for it (try tdf).",
            conflicts_with = "open"
        )]
        open_with: Option<String>,
        #[arg(
            long,
            help = "Don't wait for --open-with; let it outlive the terminal.",
            requires = "open_with",
            conflicts_with = "open"
        )]
        detach: bool,
    },
    #[command(
        about = "Compile and run locally; stdin/stdout pass through, so pipes and redirects work."
    )]
    Run {
        #[arg(value_parser = existing_file)]
        file: PathBuf,
    },
    #[command(about = "Run a file against the problem's testcases without spending a submission.")]
    Test {
        #[arg(value_parser = existing_file)]
        file: PathBuf,
        #[arg(
            short,
            long,
            help = "Problem name or id.",
            add = ArgValueCandidates::new(complete_problem)
        )]
        problem: Option<String>,
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
    #[command(about = "Open the grader in your browser (default: the problem list).")]
    Open {
        #[arg(help = "Problem name or id.", add = ArgValueCandidates::new(complete_problem))]
        problem: Option<String>,
        #[arg(
            short,
            long,
            help = "Open this submission instead.",
            conflicts_with = "problem"
        )]
        submission: Option<i64>,
    },
    #[command(about = "Compare a file with the source you last submitted.")]
    Diff {
        #[arg(value_parser = existing_file)]
        file: PathBuf,
        #[arg(
            short,
            long,
            help = "Problem name or id.",
            add = ArgValueCandidates::new(complete_problem)
        )]
        problem: Option<String>,
    },
    #[command(about = "List every attempt you have made at a problem.")]
    History {
        #[arg(help = "Problem name or id.", add = ArgValueCandidates::new(complete_problem))]
        problem: String,
    },
    #[command(about = "Print the source you submitted (default: latest for -p).")]
    Get {
        #[arg(help = "Submission id.")]
        submission: Option<i64>,
        #[arg(
            short,
            long,
            help = "Problem name or id.",
            add = ArgValueCandidates::new(complete_problem)
        )]
        problem: Option<String>,
        #[arg(
            short,
            long,
            value_name = "FILE",
            help = "Write to this file instead of stdout."
        )]
        output: Option<PathBuf>,
        #[arg(long, help = "Overwrite an existing file.", requires = "output")]
        force: bool,
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
