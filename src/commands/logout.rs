use crate::client;
use crate::config;
use crate::ui::{bold, dim, ebold, fail};

// the token is the only thing worth stealing that kafae keeps, and dropping one account's
// is not the same as dropping every account's — clean --all is the blunt instrument
pub fn run(all: bool) {
    if all {
        if client::clear_sessions() == 0 {
            println!("{}", dim("no sessions to forget"));
            return;
        }
        println!("logged out of {}", bold("every grader"));
        return;
    }

    let Some(account) = client::current_account() else {
        let config = config::load();
        if let Some(name) = config.unknown_selection() {
            fail(&format!(
                "no grader named {}, configured: {}",
                ebold(&name),
                config.grader_names().join(", ")
            ));
        }
        fail(&format!(
            "no grader to log out of, run {}",
            ebold("kafae graders")
        ));
    };

    let who = dim(format!("({})", account.login));
    if client::forget_session(account) == 0 {
        println!("{}", dim(format!("not logged in to {}", account.url)));
        return;
    }
    println!("logged out of {} {who}", bold(&account.url));
}
