use colored::Colorize;

pub fn execute(server: Option<&str>) {
    println!(
        "{} {} is not yet implemented.",
        "!".yellow().bold(),
        "berth stop".bold()
    );
    match server {
        Some(name) => println!("  Would stop server: {}", name.cyan()),
        None => println!("  Would stop all running servers."),
    }
}
