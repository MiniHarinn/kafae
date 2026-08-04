use crate::client::{clear_cache, clear_state};
use crate::ui::{bold, dim};

pub fn run(all: bool) {
    let mut freed = clear_cache();
    if all {
        freed += clear_state();
    }
    if freed == 0 {
        println!("{}", dim("nothing to clean"));
        return;
    }
    let size = if freed >= 1024 {
        format!("{:.0} KiB", freed as f64 / 1024.0)
    } else {
        format!("{freed} B")
    };
    println!("cleaned {}", bold(size));
}
