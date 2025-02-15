use std::io::Error;
use std::{env, fs::File};

use clap::{CommandFactory, ValueEnum};
use clap_complete::{generate_to, Shell};
use clap_mangen::Man;

include!("src/cli.rs");

fn main() -> Result<(), Error> {
    let outdir: PathBuf = match env::var_os("OUT_DIR") {
        None => return Ok(()),

        Some(outdir) => outdir,
    }
    .into();

    let complete_dir = outdir.join("complete");
    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_owned();
    for &shell in Shell::value_variants() {
        generate_to(shell, &mut cmd, &bin_name, &complete_dir)?;
    }

    // man page
    let mut manpage_out = File::create(outdir.join(format!("man/{bin_name}.1")))?;
    let manpage = Man::new(cmd);
    manpage.render(&mut manpage_out)?;

    Ok(())
}
