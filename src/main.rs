use std::process::ExitCode;

use clap::Parser;

use agent_change_control::{cli::Cli, execute, report::ErrorReport};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let format = cli.output_format();

    match execute(cli) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            if format.is_json() {
                let report = ErrorReport::from_error(&error);
                match serde_json::to_string_pretty(&report) {
                    Ok(json) => println!("{json}"),
                    Err(_) => eprintln!("error: {error}"),
                }
            } else {
                eprintln!("error: {error}");
            }
            ExitCode::from(error.exit_code())
        }
    }
}
