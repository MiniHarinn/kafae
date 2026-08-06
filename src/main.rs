mod cli;
mod client;
mod commands;
mod compile;
mod opener;
mod templates;
mod ui;

fn main() {
    // rust masks SIGPIPE, so a table piped into head panics instead of stopping
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    clap_complete::CompleteEnv::with_factory(cli::command).complete();
    cli::run();
}
