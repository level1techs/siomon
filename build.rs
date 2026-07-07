use clap::CommandFactory;
use clap_complete::aot::{Shell, generate_to};

include!("src/opts.rs");

fn main() -> std::io::Result<()> {
    println!("cargo::rerun-if-changed=src/opts.rs");
    println!("cargo::rerun-if-changed=build.rs");

    let outdir = std::env::var("CARGO_TARGET_DIR").unwrap_or("./target".to_string());
    let mut cmd = Cli::command();

    std::fs::create_dir_all(&outdir)?;
    generate_to(Shell::Bash, &mut cmd, "sio", &outdir)?;
    generate_to(Shell::Zsh, &mut cmd, "sio", &outdir)?;
    generate_to(Shell::Fish, &mut cmd, "sio", &outdir)?;

    println!("cargo:warning=shell completions generated in {outdir}/");
    Ok(())
}
