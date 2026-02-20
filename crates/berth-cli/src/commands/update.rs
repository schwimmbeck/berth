use colored::Colorize;

pub fn execute(server: Option<&str>, all: bool) {
    println!(
        "{} {} is not yet implemented.",
        "!".yellow().bold(),
        "berth update".bold()
    );
    if all {
        println!("  Would update all installed servers.");
    } else if let Some(name) = server {
        println!("  Would update server: {}", name.cyan());
    } else {
        println!("  Specify a server name or use --all.");
    }
}
