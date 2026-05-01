use clap::Parser;

/// lazydap daemon (placeholder — real one lands in M5).
#[derive(Parser, Debug)]
#[command(name = "lazydap-daemon", version, about)]
struct Args {
    /// Greeting message to print.
    #[arg(long, default_value = "hello from lazydap-daemon")]
    message: String,

    /// Number of times to repeat the greeting.
    #[arg(long, default_value_t = 1)]
    count: u32,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    for _ in 0..args.count {
        println!("{}", args.message);
    }
}
