#![deny(unsafe_code)]
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "donder",
    version,
    about = "Check, copy and export local Donder projects, and generate language references"
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
    /// Render a sequence for every output of the active setup into an FSEQ v2 file for FPP.
    ExportFseq {
        /// The sequence's declared name.
        sequence: String,
        output: Utf8PathBuf,
        /// Milliseconds between frames; defaults to the step closest to the sequence's frame rate.
        #[arg(long)]
        step_ms: Option<u8>,
    },
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
        Command::ExportFseq {
            sequence,
            output,
            step_ms,
        } => export_fseq(&cli.path, &sequence, &output, step_ms),
        Command::Lsp => lsp(),
        Command::Builtins { output } => {
            let text = donder_language::compiler::builtin_reference();
            donder_project_io::atomic_write(&output, text.as_bytes())
                .map_err(|error| error.to_string())?;
            println!("Wrote {output}");
            Ok(())
        }
    }
}

/// Write the FSEQ file and print the channel range of each output, so FPP's
/// channel outputs can be configured to match.
fn export_fseq(
    path: &camino::Utf8Path,
    name: &str,
    output: &camino::Utf8Path,
    step_ms: Option<u8>,
) -> Result<(), String> {
    let session = donder_project_io::load_project(path).map_err(|error| error.to_string())?;
    let project = &session.project;
    let ids = project
        .root()
        .sequences
        .iter()
        .map(|source| source.id())
        .collect::<Vec<_>>();
    let id = ids
        .iter()
        .find(|id| id.0.root_source().object() == name)
        .ok_or_else(|| {
            let names = ids
                .iter()
                .map(|id| id.0.root_source().object())
                .collect::<Vec<_>>();
            format!(
                "No sequence named `{name}`. Sequences: {}",
                names.join(", ")
            )
        })?;
    let sequence = project.sequence(id).ok_or("Sequence is missing.")?;
    let step = match step_ms {
        Some(millis) => {
            donder_output::FseqStep::from_millis(millis).ok_or("--step-ms must be at least 1.")?
        }
        None => donder_output::FseqStep::nearest(sequence.frame_rate),
    };
    let prepared =
        donder_elaboration::prepare(project, id, donder_elaboration::PrepareOutputs::All)
            .ok_or("The sequence cannot be played on the active setup.")?;
    let setup = project
        .setup(project.root().setup.id())
        .ok_or("Project setup is missing.")?;
    let mut first = 1;
    let mut channels = Vec::new();
    for prepared_output in prepared.outputs() {
        let controller = setup
            .controllers
            .get(prepared_output.controller_index as usize)
            .and_then(|source| project.controller(source.id()))
            .ok_or("Setup controller is missing.")?;
        let port = controller
            .ports
            .iter()
            .find(|port| port.id.0 == prepared_output.port)
            .ok_or("Controller port is missing.")?;
        let last = first + prepared_output.width - 1;
        channels.push(format!(
            "  channels {first}-{last}: {}#{} / {}",
            controller.id.0.document(),
            controller.id.0.root_source().object(),
            port.name.as_str()
        ));
        first = last + 1;
    }
    let media = session
        .audio_asset(id.0.document_id(), &sequence.audio)
        .and_then(|asset| asset.relative_path.file_name());
    let bytes =
        donder_output::encode_fseq(prepared, step, media).map_err(|error| error.to_string())?;
    donder_project_io::atomic_write(output, &bytes).map_err(|error| error.to_string())?;
    println!(
        "Wrote {output} at {} ms per frame, {} channels:",
        step.millis(),
        first - 1
    );
    for line in channels {
        println!("{line}");
    }
    Ok(())
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
