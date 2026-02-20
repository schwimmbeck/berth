use colored::Colorize;

pub fn execute(server: &str, tail: u32) {
    println!(
        "{} {} is not yet implemented.",
        "!".yellow().bold(),
        "berth logs".bold()
    );
    println!(
        "  Would stream logs for server: {} (last {} lines)",
        server.cyan(),
        tail
    );
}
