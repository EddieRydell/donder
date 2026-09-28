#![deny(unsafe_code)]
use camino::{Utf8Path, Utf8PathBuf};
use clap::{Parser, Subcommand};
use donder_project_io::{PROJECT_CONFIG_FILE, ProjectConfig};

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
    /// Create local configuration for an existing project document.
    Init {
        #[arg(long, default_value = "project.donder")]
        entrypoint: Utf8PathBuf,
    },
    /// Validate the project's local imports, definitions, targets, and assets.
    Check,
    /// Copy the loaded project and referenced assets into a new folder.
    Copy { destination: Utf8PathBuf },
}

pub fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Init { entrypoint } => init(&cli.path, entrypoint),
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
                "Copied {} documents/configuration files and {} assets to {}",
                report.written_files.len(),
                report.copied_assets.len(),
                destination
            );
            Ok(())
        }
    }
}

fn init(root: &Utf8Path, entrypoint: Utf8PathBuf) -> Result<(), String> {
    if root.join(PROJECT_CONFIG_FILE).exists() {
        return Err(format!("{} already exists", root.join(PROJECT_CONFIG_FILE)));
    }
    let config = ProjectConfig::new(entrypoint);
    config.validate()?;
    if !root.join(&config.entrypoint).is_file() {
        return Err(format!(
            "Entrypoint does not exist: {}",
            root.join(&config.entrypoint)
        ));
    }
    config.write(root)?;
    println!("Initialized {}", root.join(PROJECT_CONFIG_FILE));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cli_has_local_workflows_and_no_dependency_commands() {
        for args in [
            vec!["donder", "init"],
            vec!["donder", "check"],
            vec!["donder", "copy", "destination"],
        ] {
            assert!(Cli::try_parse_from(args).is_ok());
        }
        for command in [
            "sync", "update", "add", "remove", "tree", "fork", "publish", "login",
        ] {
            assert!(Cli::try_parse_from(["donder", command]).is_err());
        }
    }
    #[test]
    fn init_writes_only_local_configuration() {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(temp.path()).unwrap();
        std::fs::write(root.join("project.donder"), "show: {}\n").unwrap();
        init(root, "project.donder".into()).unwrap();
        assert_eq!(
            ProjectConfig::read(root).unwrap().entrypoint,
            "project.donder"
        );
        assert_eq!(std::fs::read_dir(root).unwrap().count(), 2);
        assert!(init(root, "project.donder".into()).is_err());
    }
}
