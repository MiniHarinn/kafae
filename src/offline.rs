use std::sync::atomic::{AtomicBool, Ordering};

use crate::ui::fail_as;

static ON: AtomicBool = AtomicBool::new(false);

pub fn enable() {
    ON.store(true, Ordering::Relaxed);
}

pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

// offline is a promise not to open a socket, so what needs one says so instead of trying
pub fn refuse(what: &str) -> ! {
    fail_as(
        "offline",
        &format!("{what} needs the grader; drop --offline or unset KAFAE_OFFLINE"),
        None,
    )
}
