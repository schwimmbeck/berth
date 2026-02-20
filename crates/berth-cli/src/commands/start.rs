use colored::Colorize;

pub fn execute(server: Option<&str>) {
    println!(
        "{} {} is not yet implemented.",
        "!".yellow().bold(),
        "berth start".bold()
    );
    match server {
        Some(name) => println!("  Would start server: {}", name.cyan()),
        None => println!("  Would start all configured servers."),
    }
}
