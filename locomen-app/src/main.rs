use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about)]
pub struct Cli {
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>
}

fn main() {
    let cli = Cli::parse();
    
    if let Some(config_path) = cli.config {
        println!("config file: {}", config_path.display());
    }
}
