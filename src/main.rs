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
    // windows has no SIGPIPE at all, so the same closed pipe comes back out of println
    #[cfg(windows)]
    {
        let report = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic| {
            let printing = panic
                .payload()
                .downcast_ref::<String>()
                .is_some_and(|message| message.starts_with("failed printing to std"));
            if printing {
                std::process::exit(0);
            }
            report(panic);
        }));
    }
    clap_complete::CompleteEnv::with_factory(cli::command).complete();
    cli::run();
}
