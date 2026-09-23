fn main() {
    if let Err(error) = donder_cli::run(donder_cli::Cli::parse_args()) {
        eprintln!("donder: {error}");
        std::process::exit(1);
    }
}
