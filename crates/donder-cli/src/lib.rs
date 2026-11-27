#![deny(unsafe_code)]
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use core::time::Duration;

mod render;
mod test;

#[derive(Debug, Parser)]
#[command(
    name = "donder",
    version,
    about = "Check, render and export local Donder projects, and generate language references"
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
    /// Render a sequence for every controller of the project with an FSEQ channel into an FSEQ v2 file for FPP.
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
        /// A time to sample in seconds, like `12.5s` or `12.5`; repeat for several.
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
    /// Put a show's layout, controllers and routes in this new project, from
    /// an xLights show folder or a Vixen profile folder.
    Import {
        profile: Utf8PathBuf,
        /// Meters between neighbouring pixels, which sets the layout's size;
        /// defaults to an xLights layout's ruler, or else 7.5 cm.
        #[arg(long)]
        pixel_spacing: Option<f64>,
        /// The Vixen preview to place shapes from, when the profile has
        /// several enabled.
        #[arg(long)]
        preview: Option<String>,
    },
    /// Play an xLights sequence (`.xsq` or `.xsqz`) on this project's fixtures as a new
    /// sequence, through a mapping of its rows to fixtures and groups. Without
    /// a mapping file, writes one to fill in.
    ImportSequence {
        /// The `.xsq` file, or an `.xsqz` package holding one.
        xsq: Utf8PathBuf,
        /// The new sequence's name.
        #[arg(long)]
        name: String,
        /// Which fixtures and groups each xLights row plays on.
        #[arg(long)]
        map: Utf8PathBuf,
        /// The song, relative to the project root.
        #[arg(long)]
        audio: Option<String>,
    },
    /// Write the generated effect-language builtin reference.
    Builtins { output: Utf8PathBuf },
    /// Stream test patterns to the project's E1.31 and Art-Net controllers,
    /// reading one per line from standard input until it ends.
    #[command(after_help = test::HELP)]
    Test,
    /// Run the language server over standard input and output.
    Lsp,
}

/// The project at `path` with the audio its effects read analyzed and their
/// fonts rasterized, or why it does not load.
fn load(path: &camino::Utf8Path) -> Result<donder_project_io::ProjectSession, String> {
    let mut session =
        donder_project_io::check_project(path, &Default::default(), &mut Default::default())
            .into_session()
            .map_err(|diagnostics| diagnostics_text(&diagnostics))?;
    let mut files: Vec<(donder_model::AssetId, Vec<donder_model::Derivation>)> = Vec::new();
    for derivation in session.project().pending_derivations() {
        match files
            .iter_mut()
            .find(|(asset, _)| asset == derivation.asset())
        {
            Some((_, derivations)) => derivations.push(derivation),
            None => files.push((derivation.asset().clone(), vec![derivation])),
        }
    }
    for (asset, derivations) in files {
        let asset = session.asset(&asset).ok_or("an asset is missing")?;
        let Some(path) = asset.available_path().map(ToOwned::to_owned) else {
            let reason = asset
                .unavailable_reason()
                .unwrap_or_else(|| unreachable!("the asset has no available path"));
            for derivation in derivations {
                session
                    .with_derived(derivation, donder_model::Derived::Failed(reason.into()))
                    .map_err(|error| error.to_string())?;
            }
            continue;
        };
        let results = donder_elaboration::derive(path.as_std_path(), &derivations);
        for (derivation, derived) in derivations.into_iter().zip(results) {
            if let donder_model::Derived::Failed(reason) = &derived {
                return Err(format!("Could not read {path}: {reason}"));
            }
            session
                .with_derived(derivation, derived)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(session)
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
                session.source().documents().len()
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
        Command::Test => test::run(&load(&cli.path)?),
        Command::Lsp => lsp(),
        Command::Import {
            profile,
            pixel_spacing,
            preview,
        } => {
            let mut session = load(&cli.path)?;
            let report = donder_import::import_profile(
                &mut session,
                &profile,
                &donder_import::ImportOptions {
                    pixel_spacing,
                    preview,
                },
            )
            .map_err(|error| error.to_string())?;
            donder_project_io::save_project(&session).map_err(|error| error.to_string())?;
            print!("{report}");
            Ok(())
        }
        Command::ImportSequence {
            xsq,
            name,
            map,
            audio,
        } => {
            let mut session = load(&cli.path)?;
            let options = donder_import::SequenceImportOptions {
                sequence: xsq,
                mapping: map,
                name,
                audio,
            };
            match donder_import::import_sequence(&mut session, &options)
                .map_err(|error| error.to_string())?
            {
                donder_import::SequenceImport::MappingStarted(path) => {
                    println!(
                        "Wrote {path}: put each xLights row's targets after its name, then import again."
                    );
                }
                donder_import::SequenceImport::Imported(report) => {
                    donder_project_io::save_project(&session).map_err(|error| error.to_string())?;
                    print!("{report}");
                }
            }
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
        .project()
        .reusable::<donder_model::Sequence>()
        .keys()
        .map(|&id| (id, session.address_of(id).object().to_string()))
        .collect::<Vec<_>>();
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
    let project = session.project();
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
    if !step.is_smooth() {
        println!(
            "warning: {} ms per frame plays less smoothly than live output; use {} ms or less",
            step.millis(),
            donder_output::FseqStep::SMOOTH_MILLIS
        );
    }
    let controllers = project.controllers().collect::<Vec<_>>();
    let placed = controllers
        .iter()
        .filter(|controller| controller.fseq_channel().is_some())
        .map(|controller| controller.id())
        .collect::<Vec<_>>();
    if placed.is_empty() {
        return Err("No controller of the project has an FSEQ channel.".into());
    }
    let prepared = donder_elaboration::prepare(
        project,
        id,
        donder_elaboration::Retain::Controllers(&placed),
    )
    .ok_or("The sequence cannot be prepared.")?;
    let fseq_channel = donder_output::setup_fseq_channel(project);
    let mut labels = Vec::new();
    for prepared_output in prepared.outputs() {
        let Some(controller) = controllers.get(prepared_output.controller_index as usize) else {
            unreachable!("the show was prepared on the project's controllers");
        };
        let output = controller.output_label(controller.runtime_output(prepared_output.port));
        let first = fseq_channel(prepared_output)
            .ok_or("A controller has no FSEQ channel.")?
            .get();
        let address = session.address_of(controller.id());
        labels.push((
            first,
            format!("{}#{} / {}", address.document(), address.object(), output),
            prepared_output.width,
        ));
    }
    let media = session
        .audio_asset(&sequence.audio)
        .filter(|asset| asset.is_available())
        .and_then(|asset| asset.relative_path.file_name());
    let bytes = donder_output::encode_fseq(prepared, fseq_channel, step, media, |_, _| {})
        .map_err(|error| error.describe(|index| &labels[index].1).to_string())?;
    donder_project_io::atomic_write(output, &bytes).map_err(|error| error.to_string())?;
    println!("Wrote {output} at {} ms per frame:", step.millis());
    labels.sort_by_key(|(first, _, _)| *first);
    for (first, label, width) in labels {
        println!("  channels {first}-{}: {label}", first + width - 1);
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
