use std::process::ExitCode;

/// Thin entrypoint. Everything worth testing lives in the library.
#[tokio::main]
async fn main() -> ExitCode {
    lazydap_daemon::run_cli(std::env::args().collect()).await
}
