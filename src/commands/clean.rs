use bytesize::ByteSize;

use crate::client::{clear_cache, clear_problem_cache, clear_state};
use crate::ui::{bold, dim};

pub fn run(all: bool, problem: Option<&str>) {
    let freed = match problem {
        Some(name) => clear_problem_cache(name),
        None => {
            let mut freed = clear_cache();
            if all {
                freed += clear_state();
            }
            freed
        }
    };
    if freed == 0 {
        println!(
            "{}",
            dim(match problem {
                Some(name) => format!("nothing cached for {name}"),
                None => "nothing to clean".to_string(),
            })
        );
        return;
    }
    let size = ByteSize::b(freed).display().iec().to_string();
    match problem {
        Some(name) => println!("cleaned {} for {}", bold(size), bold(name)),
        None => println!("cleaned {}", bold(size)),
    }
}
