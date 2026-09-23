use serde_json::json;

use crate::client::state_for_reads;
use crate::json;
use crate::ui::{ebold, fail_as};

// the token is exactly what an Authorization: Bearer header wants, so it goes to
// stdout alone and $(kafae token) is the whole of it
pub fn run() {
    let state = state_for_reads();
    let Some(token) = state.token else {
        // online, state_for_reads has already offered a password; offline it cannot
        fail_as(
            "auth",
            &format!("not logged in, run {}", ebold("kafae login")),
            None,
        )
    };

    if json::on() {
        json::emit(&json!({
            "token": token,
            "url": state.url,
            "login": state.login,
        }));
        return;
    }
    println!("{token}");
}
