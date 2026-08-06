use bytesize::ByteSize;

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
    let size = ByteSize::b(freed).display().iec().to_string();
    println!("cleaned {}", bold(size));
}
