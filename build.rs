use std::env;
use std::error::Error;
use std::fs::File;
use std::fs::create_dir_all;

use clap::CommandFactory;
use clap_complete::{Shell, generate_to};
use clap_mangen::Man;

include!("src/cli_types.rs");

fn main() -> Result<(), Box<dyn Error>> {
    // 补全内容由 clap 定义决定；这两个文件变了才需要重新生成。
    println!("cargo:rerun-if-changed=src/cli_types.rs");
    println!("cargo:rerun-if-changed=build.rs");
    // 环境变量本身也要触发重跑，否则先 cargo build 再 SHELL_HELP_DIR=... cargo build
    // 会命中缓存、不生成补全。
    println!("cargo:rerun-if-env-changed=SHELL_HELP_DIR");

    let outdir: PathBuf = match env::var_os("SHELL_HELP_DIR")
        .filter(|value| !value.is_empty())
        .or_else(|| env::var_os("OUT_DIR"))
    {
        None => return Ok(()),

        Some(outdir) => outdir,
    }
    .into();
    // let outdir: PathBuf = ".".into();
    println!("outdir: {}", outdir.display());

    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_owned();

    // complete
    let complete_dir = outdir.join("complete");
    create_dir_all(&complete_dir)?;
    let shells = [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::Elvish];
    for shell in shells {
        generate_to(shell, &mut cmd, &bin_name, &complete_dir)?;
    }

    // man page
    let man_dir = outdir.join("man");
    create_dir_all(&man_dir)?;
    let mut manpage_out = File::create(man_dir.join(format!("{bin_name}.1")))?;
    let manpage = Man::new(cmd);
    manpage.render(&mut manpage_out)?;

    Ok(())
}
