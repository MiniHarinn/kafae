use crate::templates;
use crate::ui::dim;

pub fn run() {
    for (file, builtin) in templates::listing() {
        if builtin {
            println!("{file}  {}", dim("builtin"));
        } else {
            println!("{file}");
        }
    }
    println!(
        "{}",
        dim(format!("yours go in {}", templates::user_dir().display()))
    );
}
