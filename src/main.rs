mod cli;
mod client;
mod commands;
mod compile;
mod json;
mod opener;
mod templates;
mod ui;

// std stringifies the io error into the message, and only the errno survives a locale
#[cfg(windows)]
const CLOSED_PIPE: [&str; 2] = ["os error 232", "os error 233"];
#[cfg(not(windows))]
const CLOSED_PIPE: [&str; 1] = ["os error 32"];

fn ends_the_output(message: &str) -> bool {
    message.starts_with("failed printing to std")
        && CLOSED_PIPE.iter().any(|end| message.contains(end))
}

fn main() {
    // sigpipe stays masked, so a closed pipe lands here instead of killing a test run
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        let done = panic
            .payload()
            .downcast_ref::<String>()
            .is_some_and(|message| ends_the_output(message));
        if done {
            std::process::exit(0);
        }
        report(panic);
    }));
    clap_complete::CompleteEnv::with_factory(cli::command).complete();
    cli::run();
}

#[cfg(test)]
mod tests {
    use super::*;

    // the real messages, taken from a run on each platform
    #[test]
    fn knows_a_closed_pipe_from_a_failed_write() {
        let closed = if cfg!(windows) {
            "failed printing to stdout: Pipe not connected. (os error 233)"
        } else {
            "failed printing to stdout: Broken pipe (os error 32)"
        };
        assert!(ends_the_output(closed));
        assert!(!ends_the_output(
            "failed printing to stdout: No space left on device (os error 28)"
        ));
        assert!(!ends_the_output("index out of bounds: the len is 3"));
    }
}
