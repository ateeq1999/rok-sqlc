use std::process::ExitCode;

fn main() -> ExitCode {
    match rok_sqlc::cli::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // Usage errors carry clap's own formatted message, prefix included.
            match err.downcast_ref::<clap::Error>() {
                Some(usage) => {
                    let _ = usage.print();
                }
                None => eprintln!("Error: {err:?}"),
            }
            ExitCode::FAILURE
        }
    }
}
