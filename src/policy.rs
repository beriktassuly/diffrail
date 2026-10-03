use std::path::Path;

use serde::Serialize;

use crate::{
    config::{Config, Patterns, Task},
    error::AppResult,
    git::ChangedPath,
};

#[derive(Debug)]
pub struct Evaluation {
    pub changes: Vec<ChangedPath>,
    pub violations: Vec<Violation>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Violation {
    pub path: String,
    pub code: ViolationCode,
    pub message: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_pattern: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViolationCode {
    ProtectedPath,
    SharedPath,
    OutsideScope,
}

pub fn evaluate(
    config: &Config,
    task: &Task,
    config_path: &Path,
    changes: Vec<ChangedPath>,
) -> AppResult<Evaluation> {
    let protected = Patterns::compile("policy.protected", &config.policy.protected)?;
    let shared = Patterns::compile("policy.shared", &config.policy.shared)?;
    let allow = Patterns::compile("task.allow", &task.allow)?;
    let allow_shared = Patterns::compile("task.allow_shared", &task.allow_shared)?;
    let allow_protected = Patterns::compile("task.allow_protected", &task.allow_protected)?;
    let config_path = config_path.to_string_lossy().replace('\\', "/");

    let mut violations = Vec::new();
    for change in &changes {
        let path = change.path.as_str();
        let implicit_policy_match = (path == config_path).then_some(config_path.as_str());
        let protected_match = implicit_policy_match.or_else(|| protected.matching_pattern(path));

        if let Some(pattern) = protected_match {
            if allow_protected.matching_pattern(path).is_none() {
                violations.push(Violation {
                    path: path.to_owned(),
                    code: ViolationCode::ProtectedPath,
                    message: "protected path is not allowed for this task".to_owned(),
                    matched_pattern: Some(pattern.to_owned()),
                });
            }
            continue;
        }

        if let Some(pattern) = shared.matching_pattern(path) {
            if allow_shared.matching_pattern(path).is_none() {
                violations.push(Violation {
                    path: path.to_owned(),
                    code: ViolationCode::SharedPath,
                    message: "shared contract path is not allowed for this task".to_owned(),
                    matched_pattern: Some(pattern.to_owned()),
                });
            }
            continue;
        }

        if allow.matching_pattern(path).is_none() {
            violations.push(Violation {
                path: path.to_owned(),
                code: ViolationCode::OutsideScope,
                message: "path is outside the task boundary".to_owned(),
                matched_pattern: None,
            });
        }
    }

    Ok(Evaluation {
        changes,
        violations,
    })
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::Path};

    use crate::config::{RepositoryPolicy, Task};

    use super::*;

    fn changed(path: &str) -> ChangedPath {
        ChangedPath {
            path: path.to_owned(),
            states: vec!["unstaged:modified".to_owned()],
        }
    }

    #[test]
    fn protected_paths_take_precedence_over_regular_allow() {
        let mut tasks = BTreeMap::new();
        let task = Task {
            allow: vec!["**".to_owned()],
            ..Task::default()
        };
        tasks.insert("wide".to_owned(), task);
        let config = Config {
            version: 1,
            policy: RepositoryPolicy {
                protected: vec![".github/**".to_owned()],
                shared: vec![],
            },
            tasks,
        };
        let task = config.task("wide").unwrap();

        let result = evaluate(
            &config,
            task,
            Path::new(".agent-change-control.yml"),
            vec![changed(".github/workflows/ci.yml")],
        )
        .unwrap();

        assert_eq!(result.violations[0].code, ViolationCode::ProtectedPath);
    }

    #[test]
    fn policy_file_is_always_protected() {
        let mut tasks = BTreeMap::new();
        let task = Task {
            allow: vec!["**".to_owned()],
            ..Task::default()
        };
        tasks.insert("wide".to_owned(), task);
        let config = Config {
            version: 1,
            policy: RepositoryPolicy::default(),
            tasks,
        };
        let task = config.task("wide").unwrap();

        let result = evaluate(
            &config,
            task,
            Path::new(".agent-change-control.yml"),
            vec![changed(".agent-change-control.yml")],
        )
        .unwrap();

        assert_eq!(result.violations[0].code, ViolationCode::ProtectedPath);
    }

    #[test]
    fn shared_allow_is_separate_from_regular_allow() {
        let mut tasks = BTreeMap::new();
        let task = Task {
            allow: vec!["src/**".to_owned()],
            allow_shared: vec!["src/contracts/approved.rs".to_owned()],
            ..Task::default()
        };
        tasks.insert("contract".to_owned(), task);
        let config = Config {
            version: 1,
            policy: RepositoryPolicy {
                protected: vec![],
                shared: vec!["src/contracts/**".to_owned()],
            },
            tasks,
        };
        let task = config.task("contract").unwrap();

        let result = evaluate(
            &config,
            task,
            Path::new(".agent-change-control.yml"),
            vec![
                changed("src/contracts/approved.rs"),
                changed("src/contracts/other.rs"),
            ],
        )
        .unwrap();

        assert_eq!(result.violations.len(), 1);
        assert_eq!(result.violations[0].path, "src/contracts/other.rs");
        assert_eq!(result.violations[0].code, ViolationCode::SharedPath);
    }
}
