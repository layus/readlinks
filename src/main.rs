extern crate clap;
extern crate readlinks;

use clap::{App, Arg};
use readlinks::*;
use std::io::IsTerminal;

fn main() {
    let matches = App::new("readlinks")
        .version("0.1.0")
        .author("Guillaume Maudoux (layus) <layus.on@gmail.com>")
        .about("readlinks, the pedantic symlink resolver.")
        .arg(Arg::with_name("path")
            .help("An executable reachable through $PATH, or a filesystem path.")
            .value_name("executable|path")
            .required(true)
        )
        .arg(Arg::with_name("verbose")
            .short("v")
            .long("verbose")
            .help("Show each symlink resolution step")
        )
        .get_matches();

    let path = expand_path(matches.value_of("path").unwrap());
    let verbose = matches.is_present("verbose");

    let color = std::io::stdout().is_terminal();
    let resolution = resolve(path);            // pass 1: resolve the symlink chain
    let rows = layout(&resolution, verbose);   // pass 2: compute aligned offsets
    for line in render(&rows, color) {          // pass 3: render the lines
        println!("{}", line);
    }
}
