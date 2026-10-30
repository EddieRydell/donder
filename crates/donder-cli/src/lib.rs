#![deny(unsafe_code)]
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use core::time::Duration;

mod render;

#[derive(Debug, Parser)]
#[command(
    name = "donder",
    version,
    about = "Check, copy, render and export local Donder projects, and generate language references"
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
    /// Print each fixture's colors at the given times of a sequence, with
    /// repeated colors written once as `#rrggbb×count`.
    Render {
        /// The sequence's declared name.
        sequence: String,
        /// A time to sample, like `12.5s`; repeat for several.
        #[arg(long, value_parser = donder_language::data::parse_duration)]
        at: Vec<Duration>,
        /// Sample every `--step` from `--from` through `--to`.
        #[arg(long, value_parser = donder_language::data::parse_duration, requires_all = ["to", "step"])]
        from: Option<Duration>,
        #[arg(long, value_parser = donder_language::data::parse_duration, requires_all = ["from", "step"])]
        to: Option<Duration>,
        #[arg(long, value_parser = donder_language::data::parse_duration, requires_all = ["from", "to"])]
        step: Option<Duration>,
        /// Only this fixture or group of the active layout; repeat for several.
        #[arg(long)]
        target: Vec<String>,
    },
    /// Put an xLights show's layout, controllers and patch in this new
    /// project, from the show folder holding `xlights_rgbeffects.xml`.
    ImportXlights {
        show: Utf8PathBuf,
        /// Meters between neighbouring pixels, which sets the layout's size;
        /// defaults to the layout's ruler, or 7.5 cm without one.
        #[arg(long)]
        pixel_spacing: Option<f64>,
    },
    /// Write the generated effect-language builtin reference.
    Builtins { output: Utf8PathBuf },
    /// Run the language server over standard input and output.
    Lsp,
}

/// The project at `path`, or why it does not load.
fn load(path: &camino::Utf8Path) -> Result<donder_project_io::ProjectSession, String> {
    donder_project_io::check_project(path, &Default::default(), &mut Default::default())
        .into_session()
        .map_err(|diagnostics| diagnostics_text(&diagnostics))
}

/// One diagnostic per line.
fn diagnostics_text(diagnostics: &[donder_project_io::IoDiagnostic]) -> String {
    let lines = diagnostics.iter().map(ToString::to_string);
    lines.collect::<Vec<_>>().join(
        "
",
    )
}

pub fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Check => {
            let report = donder_project_io::check_project(
                &cli.path,
                &Default::default(),
                &mut Default::default(),
            );
            let Some(session) = report.session else {
                return Err(diagnostics_text(&report.diagnostics));
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
            let session = load(&cli.path)?;
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
        Command::Render {
            sequence,
            at,
            from,
            to,
            step,
            target,
        } => {
            let session = load(&cli.path)?;
            let id = find_sequence(&session, &sequence)?;
            let times = render::Times {
                at,
                range: from
                    .zip(to)
                    .zip(step)
                    .map(|((from, to), step)| (from, to, step)),
            };
            print!("{}", render::render(&session, &id, times, &target)?);
            Ok(())
        }
        Command::Lsp => lsp(),
        Command::ImportXlights {
            show,
            pixel_spacing,
        } => {
            let mut session = load(&cli.path)?;
            let report = donder_xlights::import_show(
                &mut session,
                &show,
                &donder_xlights::ImportOptions { pixel_spacing },
            )
            .map_err(|error| error.to_string())?;
            donder_project_io::save_project(&session).map_err(|error| error.to_string())?;
            print!("{report}");
            Ok(())
        }
        Command::Builtins { output } => {
            let text = donder_language::compiler::builtin_reference();
            donder_project_io::atomic_write(&output, text.as_bytes())
                .map_err(|error| error.to_string())?;
            println!("Wrote {output}");
            Ok(())
        }
    }
}

/// The project's sequence named `name`, as it is declared or owned.
fn find_sequence(
    session: &donder_project_io::ProjectSession,
    name: &str,
) -> Result<donder_model::SequenceId, String> {
    let sequences = session
        .project
        .root()
        .sequences
        .iter()
        .map(|source| {
            let id = *source.id();
            session
                .address_of(id.into())
                .map(|address| (id, address.declared_name().to_string()))
                .ok_or("A sequence is missing from the source inventory.")
        })
        .collect::<Result<Vec<_>, _>>()?;
    sequences
        .iter()
        .find(|(_, declared)| declared == name)
        .map(|(id, _)| *id)
        .ok_or_else(|| {
            let names = sequences
                .iter()
                .map(|(_, declared)| declared.as_str())
                .collect::<Vec<_>>();
            format!(
                "No sequence named `{name}`. Sequences: {}",
                names.join(", ")
            )
        })
}

/// Write the FSEQ file and print the channel range of each output, so FPP's
/// channel outputs can be configured to match.
fn export_fseq(
    path: &camino::Utf8Path,
    name: &str,
    output: &camino::Utf8Path,
    step_ms: Option<u8>,
) -> Result<(), String> {
    let session = load(path)?;
    let project = &session.project;
    let id = &find_sequence(&session, name)?;
    let sequence = project
        .get::<donder_model::Sequence>(*id)
        .ok_or("Sequence is missing.")?;
    let step = match step_ms {
        Some(millis) => {
            donder_output::FseqStep::from_millis(millis).ok_or("--step-ms must be at least 1.")?
        }
        None => donder_output::FseqStep::nearest(sequence.frame_rate),
    };
    let prepared = donder_elaboration::prepare(project, id, donder_elaboration::Retain::All)
        .ok_or("The sequence cannot be played on the active setup.")?;
    let setup = project
        .get::<donder_model::Setup>(*project.root().setup.id())
        .ok_or("Project setup is missing.")?;
    let mut first = 1;
    let mut channels = Vec::new();
    for prepared_output in prepared.outputs() {
        let controller = setup
            .controllers
            .get(prepared_output.controller_index as usize)
            .and_then(|source| project.get::<donder_model::Controller>(*source.id()))
            .ok_or("Setup controller is missing.")?;
        let port = controller
            .ports
            .iter()
            .find(|port| port.id.0 == prepared_output.port)
            .ok_or("Controller port is missing.")?;
        let last = first + prepared_output.width - 1;
        let address = session
            .address_of(controller.id.into())
            .ok_or("A controller is missing from the source inventory.")?;
        channels.push(format!(
            "  channels {first}-{last}: {}#{} / {}",
            address.document(),
            address.declared_name(),
            port.name.as_str()
        ));
        first = last + 1;
    }
    let media = session
        .audio_asset(&sequence.audio)
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
