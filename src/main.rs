mod cli;
mod client;
mod commands;
mod compile;
mod templates;
mod ui;

fn main() {
    clap_complete::CompleteEnv::with_factory(cli::command).complete();
    cli::run();
}
