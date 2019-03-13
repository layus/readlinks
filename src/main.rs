extern crate clap;
extern crate readlinks;

use clap::{App, Arg};
use readlinks::*;

fn main() -> std::io::Result<()> {
    let args = App::new("readlinks")
        .version("0.1.0")
        .author("Guillaume Maudoux \"Layus\" <layus.on@gmail.com>")
        .about("readlinks, the pedantic symlink resolver")
        .arg(Arg::with_name("verbose")
            .short("v")
            .help("increase verbosity"))
        .arg(Arg::with_name("path")
             .help("the path to scrutinize") 
             .required(true))
        .get_matches();

    let verbose = args.is_present("verbose");

    if let Some(path) = args.value_of("path") {
        let path = expand_path(path);
        println!("{}", path.display());
        resolve_iter(path).for_each(|s| {
            if verbose {
                println!("{} -> {}", s.source.display(), s.target.display());
            }
            println!("{}", s.resolved.display())
        })
    }
    
    Ok(())
}
