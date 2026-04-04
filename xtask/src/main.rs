use clap::{Parser, Subcommand};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    pub(crate) command: Cmd,
}
#[derive(Subcommand)]
enum Cmd {
    ShellCompletion(ShellCompletion),
}

#[derive(Parser)]
struct ShellCompletion {
    #[clap(value_enum)]
    shell: clap_complete::Shell,
}

fn main() {
    let cli = Cli::parse();

    let mut cmd = <siomon::cli::Cli as clap::CommandFactory>::command();
    match cli.command {
        Cmd::ShellCompletion(shell_completion) => {
            let bin_name = cmd.get_name().to_string();

            clap_complete::generate(
                shell_completion.shell,
                &mut cmd,
                bin_name,
                &mut std::io::stdout(),
            );
        }
    }
}
