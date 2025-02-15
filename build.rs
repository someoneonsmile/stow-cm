use std::env;
use std::error::Error;
use std::fs::create_dir_all;
use std::fs::File;

use clap::{CommandFactory, ValueEnum};
use clap_complete::{generate_to, Shell};
use clap_mangen::Man;

include!("src/cli.rs");

fn main() -> Result<(), Box<dyn Error>> {
    let outdir: PathBuf = match env::var_os("SHELL_HELP_DIR").or_else(|| env::var_os("OUT_DIR")) {
        None => return Ok(()),

        Some(outdir) => outdir,
    }
    .into();
    // let outdir: PathBuf = ".".into();

    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_owned();

    // complete
    let complete_dir = outdir.join("complete");
    create_dir_all(&complete_dir)?;
    for &shell in Shell::value_variants() {
        generate_to(shell, &mut cmd, &bin_name, &complete_dir)?;
    }

    // man page
    let man_file_path = outdir.join(format!("man/{bin_name}.1"));
    create_dir_all(&man_file_path)?;
    let mut manpage_out = File::create(man_file_path)?;
    let manpage = Man::new(cmd);
    manpage.render(&mut manpage_out)?;

    Ok(())
}
