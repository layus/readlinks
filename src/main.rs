extern crate clap;
extern crate readlinks;

use clap::{App, Arg};
use readlinks::*;

fn main() {
    let args = App::new("readlinks")
        .version("0.1.0")
        .author("Guillaume Maudoux (layus) <layus.on@gmail.com>")
        .about("readlinks, the pedantic symlink resolver.")
        .arg(Arg::with_name("path")
            .help("An executable reachable through $PATH, or a filesystem path.")
            .value_name("executable|path")
            .required(true)
        )
        .get_matches();

    let path = expand_path(args.value_of("path").unwrap());
    resolve(path).for_each(|s| {
        println!("{}", s);
    });
}
