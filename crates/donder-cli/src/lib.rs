#![deny(unsafe_code)]
use camino::{Utf8Path, Utf8PathBuf};
use clap::{Parser, Subcommand};
use donder_project_io::{PROJECT_ROOT_FILE, ProjectMetadata};

#[derive(Debug, Parser)]
#[command(
    name = "donder",
    version,
    about = "Check and copy local Donder projects"
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
    /// Add workspace metadata to an existing root project.donder document.
    Init,
    /// Validate the project's local imports, definitions, targets, and assets.
    Check,
    /// Copy the loaded project and referenced assets into a new folder.
    Copy { destination: Utf8PathBuf },
}

pub fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Init => init(&cli.path),
        Command::Check => {
            let session =
                donder_project_io::load_project(&cli.path).map_err(|error| error.to_string())?;
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
    }
}

fn init(root: &Utf8Path) -> Result<(), String> {
    let path = root.join(PROJECT_ROOT_FILE);
    let source = std::fs::read_to_string(&path).map_err(|error| format!("{path}: {error}"))?;
    let text = ProjectMetadata::default().initialize_document(&source)?;
    donder_project_io::atomic_write(&path, text.as_bytes()).map_err(|error| error.to_string())?;
    println!("Initialized {path}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn init_adds_metadata_to_existing_root_and_rejects_reinitialization() {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(temp.path()).unwrap();
        std::fs::write(root.join("project.donder"), "show: {}\n").unwrap();
        init(root).unwrap();
        let metadata = ProjectMetadata::read(root).unwrap();
        assert!(!metadata.project_id.is_nil());
        assert!(
            std::fs::read_to_string(root.join(PROJECT_ROOT_FILE))
                .unwrap()
                .contains("show: {}")
        );
        assert_eq!(std::fs::read_dir(root).unwrap().count(), 1);
        assert!(init(root).is_err());
        assert_eq!(ProjectMetadata::read(root).unwrap(), metadata);
    }
}
