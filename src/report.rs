use std::path::Path;

use serde::Serialize;

use crate::{
    cli::OutputFormat,
    config::Config,
    error::{AppError, AppResult},
    git::ChangedPath,
    policy::{Evaluation, Violation},
};

#[derive(Debug, Serialize)]
pub struct CheckReport {
    pub schema_version: u32,
    pub ok: bool,
    pub task: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,

    pub head: String,
    pub policy_revision: String,
    pub diff_base: String,
    pub changes: Vec<ChangedPath>,
    pub violations: Vec<Violation>,
    pub summary: Summary,
}

#[derive(Debug, Serialize)]
pub struct Summary {
    pub changed: usize,
    pub allowed: usize,
    pub violations: usize,
}

impl CheckReport {
    pub fn new(
        task: String,
        base: Option<String>,
        head: String,
        policy_revision: String,
        diff_base: String,
        evaluation: Evaluation,
    ) -> Self {
        let changed = evaluation.changes.len();
        let violation_count = evaluation.violations.len();
        Self {
            schema_version: 1,
            ok: violation_count == 0,
            task,
            base,
            head,
            policy_revision,
            diff_base,
            changes: evaluation.changes,
            violations: evaluation.violations,
            summary: Summary {
                changed,
                allowed: changed.saturating_sub(violation_count),
                violations: violation_count,
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorReport {
    pub schema_version: u32,
    pub ok: bool,
    pub error: ErrorBody,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
}

impl ErrorReport {
    pub fn from_error(error: &AppError) -> Self {
        Self {
            schema_version: 1,
            ok: false,
            error: ErrorBody {
                code: error.code(),
                message: error.to_string(),
            },
        }
    }
}

pub fn render_check(format: OutputFormat, quiet: bool, report: &CheckReport) -> AppResult<()> {
    match format {
        OutputFormat::Human => render_human(quiet, report),
        OutputFormat::Json => {
            let output = serde_json::to_string_pretty(report)
                .map_err(|error| AppError::io(format!("cannot serialize report: {error}")))?;
            println!("{output}");
        }
        OutputFormat::Github => render_github(report),
    }
    Ok(())
}

pub fn render_validation(format: OutputFormat, path: &Path, config: &Config) -> AppResult<()> {
    match format {
        OutputFormat::Human | OutputFormat::Github => println!(
            "Valid policy: {} ({} tasks)",
            path.display(),
            config.tasks.len()
        ),
        OutputFormat::Json => {
            #[derive(Serialize)]
            struct Validation<'a> {
                schema_version: u32,
                ok: bool,
                policy: String,
                tasks: Vec<&'a str>,
            }
            let value = Validation {
                schema_version: 1,
                ok: true,
                policy: path.to_string_lossy().replace('\\', "/"),
                tasks: config.tasks.keys().map(String::as_str).collect(),
            };
            let output = serde_json::to_string_pretty(&value)
                .map_err(|error| AppError::io(format!("cannot serialize report: {error}")))?;
            println!("{output}");
        }
    }
    Ok(())
}

fn render_human(quiet: bool, report: &CheckReport) {
    if report.ok {
        if !quiet {
            println!(
                "PASS {}: {} changed path(s) are within scope",
                display_text(&report.task),
                report.summary.changed
            );
            println!(
                "  policy revision: {}",
                short_commit(&report.policy_revision)
            );
            println!("  diff base: {}", short_commit(&report.diff_base));
        }
        return;
    }

    println!(
        "FAIL {}: {} violation(s) across {} changed path(s)",
        display_text(&report.task),
        report.summary.violations,
        report.summary.changed
    );
    for violation in &report.violations {
        println!(
            "  [{}] {} — {}",
            serde_json::to_value(violation.code)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "violation".to_owned()),
            display_text(&violation.path),
            violation.message
        );
    }
    println!(
        "  policy revision: {}",
        short_commit(&report.policy_revision)
    );
    println!("  diff base: {}", short_commit(&report.diff_base));
}

fn render_github(report: &CheckReport) {
    for violation in &report.violations {
        println!(
            "::error file={}::agent-change-control: {}",
            github_property(&violation.path),
            github_message(&violation.message)
        );
    }
    if report.ok {
        println!(
            "PASS {}: {} changed path(s) are within scope",
            display_text(&report.task),
            report.summary.changed
        );
    } else {
        println!(
            "FAIL {}: {} policy violation(s)",
            display_text(&report.task),
            report.summary.violations
        );
    }
}

fn display_text(value: &str) -> String {
    value.chars().flat_map(char::escape_default).collect()
}

fn github_property(value: &str) -> String {
    github_message(value)
        .replace(':', "%3A")
        .replace(',', "%2C")
}

fn github_message(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn short_commit(commit: &str) -> &str {
    commit.get(..12).unwrap_or(commit)
}
