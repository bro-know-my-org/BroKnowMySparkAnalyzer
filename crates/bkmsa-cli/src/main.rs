use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    match bkmsa_cli::run_from_env().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("bkmsa: {error}");
            error.exit_code()
        }
    }
}
