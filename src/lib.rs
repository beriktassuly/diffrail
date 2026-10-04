pub mod cli;
pub mod config;
pub mod error;
pub mod git;
pub mod policy;
pub mod report;

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use cli::{Cli, Command};
use config::{Config, validate_config_path};
use error::{AppError, AppResult};
use git::Repository;
use policy::evaluate;
use report::{CheckReport, render_check, render_validation};

pub const CONFIG_TEMPLATE: &str = r#"version: 1

policy:
  protected:
    - ".diffrail.yml"
    - ".github/**"
  shared:
    - "schemas/**"
    - "src/contracts/**"

tasks:
  example:
    description: "Replace this task with the boundary for your first change"
    allow:
      - "src/**"
      - "tests/**"
    allow_shared: []
    allow_protected: []
"#;

pub fn execute(cli: Cli) -> AppResult<u8> {
    let repo = Repository::discover(cli.repo.as_deref())?;

    match cli.command {
        Command::Init { config } => init(&repo, &config),
        Command::Validate { config, format } => {
            let config_path = validate_config_path(&config)?;
            let target = repo.root().join(&config_path);
            ensure_path_stays_in_repo(repo.root(), &target)?;
            let contents = fs::read_to_string(&target).map_err(|error| {
                let message = format!("cannot read {}: {error}", config_path.display());
                match error.kind() {
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidData => {
                        AppError::config(message)
                    }
                    _ => AppError::io(message),
                }
            })?;
            let parsed = Config::parse(&contents)?;
            render_validation(format, &config_path, &parsed)?;
            Ok(0)
        }
        Command::Check {
            task,
            base,
            head,
            config,
            format,
            quiet,
        } => {
            let config_path = validate_config_path(&config)?;
            let snapshot = repo.snapshot(base.as_deref(), &head, &config_path)?;
            let parsed = Config::parse(&snapshot.config)?;
            let task_config = parsed.task(&task)?;
            let evaluation = evaluate(&parsed, task_config, &config_path, snapshot.changes)?;
            let report = CheckReport::new(
                task,
                base,
                head,
                snapshot.policy_revision,
                snapshot.diff_base,
                evaluation,
            );
            render_check(format, quiet, &report)?;
            Ok(if report.ok { 0 } else { 1 })
        }
    }
}

fn init(repo: &Repository, config: &Path) -> AppResult<u8> {
    let config_path = validate_config_path(config)?;
    let target = repo.root().join(&config_path);

    if fs::symlink_metadata(&target).is_ok() {
        return Err(AppError::config(format!(
            "{} already exists; it was not changed",
            config_path.display()
        )));
    }

    if let Some(parent) = target.parent() {
        if !parent.is_dir() {
            return Err(AppError::config(format!(
                "parent directory {} does not exist",
                parent.display()
            )));
        }
        ensure_path_stays_in_repo(repo.root(), parent)?;
    }

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|error| AppError::io(format!("cannot create {}: {error}", target.display())))?;

    if let Err(error) = file.write_all(CONFIG_TEMPLATE.as_bytes()) {
        drop(file);
        let _ = fs::remove_file(&target);
        return Err(AppError::io(format!(
            "cannot write {}: {error}",
            target.display()
        )));
    }

    println!("Created {}", config_path.display());
    println!("Edit the task boundaries, validate them, and commit the policy before use.");
    println!("Next: diffrail validate --config {}", config_path.display());
    Ok(0)
}

fn ensure_path_stays_in_repo(root: &Path, path: &Path) -> AppResult<()> {
    let canonical_root = root.canonicalize().map_err(|error| {
        AppError::io(format!(
            "cannot resolve repository root {}: {error}",
            root.display()
        ))
    })?;
    let canonical_path = path.canonicalize().or_else(|_| {
        path.parent()
            .ok_or_else(|| std::io::Error::other("path has no parent"))?
            .canonicalize()
    });
    let canonical_path = canonical_path
        .map_err(|error| AppError::io(format!("cannot resolve {}: {error}", path.display())))?;

    if !canonical_path.starts_with(&canonical_root) {
        return Err(AppError::config(
            "policy path resolves outside the repository root",
        ));
    }
    Ok(())
}
