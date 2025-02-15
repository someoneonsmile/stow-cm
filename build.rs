use std::env;
use std::io::Error;

use clap::{CommandFactory, ValueEnum};
use clap_complete::{generate_to, Shell};
include!("src/cli.rs");

fn main() -> Result<(), Error> {
    let outdir = match env::var_os("OUT_DIR") {
        None => return Ok(()),

        Some(outdir) => outdir,
    };

    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_owned();
    for &shell in Shell::value_variants() {
        generate_to(shell, &mut cmd, &bin_name, &outdir)?;
    }

    Ok(())
}
