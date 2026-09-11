use clap::Parser;
use readlinks::*;
use std::io::IsTerminal;

/// readlinks, the pedantic symlink resolver.
#[derive(Parser)]
#[command(version = "0.1.0", author = "Guillaume Maudoux (layus) <layus.on@gmail.com>")]
struct Cli {
    /// An executable reachable through $PATH, or a filesystem path.
    #[arg(value_name = "executable|path")]
    path: String,

    /// Show each symlink resolution step
    #[arg(short, long)]
    verbose: bool,
}

fn main() {
    let cli = Cli::parse();

    let path = expand_path(&cli.path);
    let verbose = cli.verbose;

    let color = std::io::stdout().is_terminal();
    let resolution = resolve(path);            // pass 1: resolve the symlink chain
    let rows = layout(&resolution, verbose);   // pass 2: compute aligned offsets
    for line in render(&rows, color) {          // pass 3: render the lines
        println!("{}", line);
    }
}
