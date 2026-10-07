#![deny(unsafe_code)]
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "donder",
    version,
    about = "Check and copy local Donder projects, and generate language references"
)]
pub struct Cli {
    #[arg(short, long, default_value = ".")]
    path: Utf8PathBuf,
    #[command(subcommand)]
    command: Command,
}
impl Cli {
    pub fn parse_args() -> Self {
        Self::parse()
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate the project's local imports, definitions, targets, and assets.
    Check,
    /// Copy the loaded project and referenced assets into a new folder.
    Copy { destination: Utf8PathBuf },
    /// Write the generated effect-language builtin reference.
    Builtins { output: Utf8PathBuf },
    /// Run the language server over standard input and output.
    Lsp,
}

pub fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Check => {
            let report = donder_project_io::check_project(&cli.path);
            let Some(session) = report.session else {
                return Err(donder_project_io::ProjectLoadError(report.diagnostics).to_string());
            };
            for warning in &report.diagnostics {
                println!("warning: {warning}");
            }
            println!(
                "Project is valid ({} loaded documents)",
                session.source.documents.len()
            );
            Ok(())
        }
        Command::Copy { destination } => {
            let session =
                donder_project_io::load_project(&cli.path).map_err(|error| error.to_string())?;
            let report = donder_project_io::export_editable_project(&session, &destination)?;
            println!(
                "Copied {} documents and {} assets to {}",
                report.written_files.len(),
                report.copied_assets.len(),
                destination
            );
            Ok(())
        }
        Command::Lsp => lsp(),
        Command::Builtins { output } => {
            let text = donder_language::dsl::builtins::builtin_reference();
            donder_project_io::atomic_write(&output, text.as_bytes())
                .map_err(|error| error.to_string())?;
            println!("Wrote {output}");
            Ok(())
        }
    }
}

/// Serve one client over stdio, rechecking when messages pause.
fn lsp() -> Result<(), String> {
    let (connection, threads) = lsp_server::Connection::stdio();
    let mut server = donder_language_server::Server::new();
    let send = |messages: Vec<serde_json::Value>| -> Result<(), String> {
        for message in messages {
            let message = serde_json::from_value::<lsp_server::Message>(message)
                .map_err(|error| error.to_string())?;
            connection
                .sender
                .send(message)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    };
    let pause = std::time::Duration::from_millis(donder_language_server::IDLE_DELAY_MS);
    while !server.exited() {
        match connection.receiver.recv_timeout(pause) {
            Ok(message) => {
                let message = serde_json::to_value(message).map_err(|error| error.to_string())?;
                send(server.handle(message))?;
            }
            Err(error) if error.is_timeout() => send(server.idle())?,
            Err(_) => break,
        }
    }
    drop(connection);
    threads.join().map_err(|error| error.to_string())
}
