use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

use globset::{GlobBuilder, GlobMatcher};
use serde::Deserialize;

use crate::error::{AppError, AppResult};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,

    #[serde(default)]
    pub policy: RepositoryPolicy,

    pub tasks: BTreeMap<String, Task>,
}

impl Config {
    pub fn parse(contents: &str) -> AppResult<Self> {
        let config: Self = serde_yaml::from_str(contents)
            .map_err(|error| AppError::config(format!("invalid policy YAML: {error}")))?;
        config.validate()?;
        Ok(config)
    }

    pub fn task(&self, id: &str) -> AppResult<&Task> {
        self.tasks.get(id).ok_or_else(|| {
            let available = self.tasks.keys().cloned().collect::<Vec<_>>().join(", ");
            AppError::config(if available.is_empty() {
                format!("task {id:?} is not defined; the policy has no tasks")
            } else {
                format!("task {id:?} is not defined; available tasks: {available}")
            })
        })
    }

    fn validate(&self) -> AppResult<()> {
        if self.version != 1 {
            return Err(AppError::config(format!(
                "unsupported policy version {}; expected 1",
                self.version
            )));
        }
        if self.tasks.is_empty() {
            return Err(AppError::config("policy must define at least one task"));
        }

        validate_patterns("policy.protected", &self.policy.protected)?;
        validate_patterns("policy.shared", &self.policy.shared)?;

        for (id, task) in &self.tasks {
            if id.trim().is_empty() {
                return Err(AppError::config("task IDs cannot be empty"));
            }
            validate_patterns(&format!("tasks.{id}.allow"), &task.allow)?;
            validate_patterns(&format!("tasks.{id}.allow_shared"), &task.allow_shared)?;
            validate_patterns(
                &format!("tasks.{id}.allow_protected"),
                &task.allow_protected,
            )?;
        }
        Ok(())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RepositoryPolicy {
    pub protected: Vec<String>,
    pub shared: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Task {
    pub description: Option<String>,
    pub allow: Vec<String>,
    pub allow_shared: Vec<String>,
    pub allow_protected: Vec<String>,
}

#[derive(Debug)]
pub struct Patterns {
    entries: Vec<(String, GlobMatcher)>,
}

impl Patterns {
    pub fn compile(label: &str, patterns: &[String]) -> AppResult<Self> {
        let mut entries = Vec::with_capacity(patterns.len());
        for pattern in patterns {
            validate_pattern(label, pattern)?;
            entries.push((pattern.clone(), compile_glob(label, pattern)?));
            if let Some(root) = pattern.strip_suffix("/**") {
                if !root.is_empty() {
                    entries.push((pattern.clone(), compile_glob(label, root)?));
                }
            }
        }
        Ok(Self { entries })
    }

    pub fn matching_pattern(&self, path: &str) -> Option<&str> {
        self.entries
            .iter()
            .find_map(|(pattern, matcher)| matcher.is_match(path).then_some(pattern.as_str()))
    }
}

pub fn validate_config_path(path: &Path) -> AppResult<PathBuf> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(AppError::config(
            "policy path must be a non-empty repository-relative path",
        ));
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_str().ok_or_else(|| {
                    AppError::config("policy path must contain valid UTF-8 characters")
                })?;
                if part.eq_ignore_ascii_case(".git") {
                    return Err(AppError::config(
                        "policy path cannot point inside Git metadata",
                    ));
                }
                if part.contains('\\') || part.contains(':') {
                    return Err(AppError::config(
                        "policy path must use portable repository-relative components",
                    ));
                }
                normalized.push(part);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(AppError::config(
                    "policy path cannot leave the repository root",
                ));
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        return Err(AppError::config("policy path cannot be empty"));
    }
    Ok(normalized)
}

fn validate_patterns(label: &str, patterns: &[String]) -> AppResult<()> {
    for pattern in patterns {
        validate_pattern(label, pattern)?;
        compile_glob(label, pattern)?;
    }
    Ok(())
}

fn compile_glob(label: &str, pattern: &str) -> AppResult<GlobMatcher> {
    GlobBuilder::new(pattern)
        .literal_separator(true)
        .backslash_escape(false)
        .case_insensitive(false)
        .build()
        .map(|glob| glob.compile_matcher())
        .map_err(|error| AppError::config(format!("invalid glob in {label}: {pattern:?}: {error}")))
}

fn validate_pattern(label: &str, pattern: &str) -> AppResult<()> {
    if pattern.is_empty() {
        return Err(AppError::config(format!("empty glob in {label}")));
    }
    if pattern.starts_with('/')
        || pattern.starts_with('\\')
        || pattern.as_bytes().get(1) == Some(&b':')
    {
        return Err(AppError::config(format!(
            "glob in {label} must be repository-relative: {pattern:?}"
        )));
    }
    if pattern.split(['/', '\\']).any(|part| part == "..") {
        return Err(AppError::config(format!(
            "glob in {label} cannot contain '..': {pattern:?}"
        )));
    }
    if pattern.contains('\\') {
        return Err(AppError::config(format!(
            "glob in {label} must use '/' separators: {pattern:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_fields() {
        let error = Config::parse(
            r#"
version: 1
policy: {}
tasks:
  docs:
    allow: ["docs/**"]
    typo: true
"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn patterns_use_repository_separators() {
        let patterns = Patterns::compile("test", &["src/**".to_owned()]).unwrap();
        assert!(patterns.matching_pattern("src").is_some());
        assert!(patterns.matching_pattern("src/lib.rs").is_some());
        assert!(patterns.matching_pattern("src2/lib.rs").is_none());
    }

    #[test]
    fn policy_path_cannot_target_git_metadata() {
        let error = validate_config_path(Path::new(".git/config")).unwrap_err();
        assert!(error.to_string().contains("Git metadata"));

        let error = validate_config_path(Path::new(".GIT/config")).unwrap_err();
        assert!(error.to_string().contains("Git metadata"));
    }
}
