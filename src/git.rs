use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde::Serialize;

use crate::error::{AppError, AppResult};

#[derive(Debug)]
pub struct Repository {
    root: PathBuf,
}

#[derive(Debug)]
pub struct Snapshot {
    pub policy_revision: String,
    pub diff_base: String,
    pub config: String,
    pub changes: Vec<ChangedPath>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChangedPath {
    pub path: String,
    pub states: Vec<String>,
}

impl Repository {
    pub fn discover(start: Option<&Path>) -> AppResult<Self> {
        let start = start.unwrap_or_else(|| Path::new("."));
        let output = Command::new("git")
            .arg("-C")
            .arg(start)
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .map_err(|error| AppError::git(format!("cannot run Git: {error}")))?;

        if !output.status.success() {
            return Err(AppError::git(format!(
                "not a Git repository: {}",
                stderr_message(&output)
            )));
        }

        let root = String::from_utf8(output.stdout)
            .map_err(|_| AppError::git("Git returned a non-UTF-8 repository path"))?;
        Ok(Self {
            root: PathBuf::from(root.trim()),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn snapshot(
        &self,
        base: Option<&str>,
        head: &str,
        config_path: &Path,
    ) -> AppResult<Snapshot> {
        if base.is_none() && head != "HEAD" {
            return Err(AppError::config("--head requires --base"));
        }

        let head_commit = self.resolve_commit(head)?;
        let (policy_revision, diff_base) = match base {
            Some(base_ref) => {
                let base_commit = self.resolve_commit(base_ref)?;
                let merge_base = self.merge_base(&base_commit, &head_commit)?;
                (base_commit, merge_base)
            }
            None => (head_commit.clone(), head_commit.clone()),
        };

        let config = self.read_file_at(&policy_revision, config_path)?;
        let mut changes = ChangeSet::default();

        if base.is_some() {
            let output = self.git_output([
                "diff",
                "--name-status",
                "-z",
                "--find-renames",
                &diff_base,
                &head_commit,
            ])?;
            changes.extend(parse_name_status(&output.stdout, "committed")?);
        }

        let current_head = self.resolve_commit("HEAD")?;
        if head_commit == current_head {
            let staged = self.git_output([
                "diff",
                "--cached",
                "--name-status",
                "-z",
                "--find-renames",
                &current_head,
            ])?;
            changes.extend(parse_name_status(&staged.stdout, "staged")?);

            let unstaged = self.git_output(["diff", "--name-status", "-z", "--find-renames"])?;
            changes.extend(parse_name_status(&unstaged.stdout, "unstaged")?);

            let untracked =
                self.git_output(["ls-files", "--others", "--exclude-standard", "-z"])?;
            changes.extend(parse_untracked(&untracked.stdout)?);
        }

        Ok(Snapshot {
            policy_revision,
            diff_base,
            config,
            changes: changes.into_paths(),
        })
    }

    fn resolve_commit(&self, revision: &str) -> AppResult<String> {
        if revision.trim().is_empty() {
            return Err(AppError::config("Git revision cannot be empty"));
        }
        let peeled = format!("{revision}^{{commit}}");
        let output =
            self.git_output_raw(["rev-parse", "--verify", "--end-of-options", peeled.as_str()])?;
        if !output.status.success() {
            return Err(AppError::git(format!(
                "cannot resolve revision {revision:?} as a commit: {}",
                stderr_message(&output)
            )));
        }
        let value = String::from_utf8(output.stdout)
            .map_err(|_| AppError::git("Git returned a non-UTF-8 commit ID"))?;
        Ok(value.trim().to_owned())
    }

    fn merge_base(&self, base: &str, head: &str) -> AppResult<String> {
        let output = self.git_output_raw(["merge-base", base, head])?;
        if !output.status.success() {
            return Err(AppError::git(format!(
                "cannot find a merge-base; fetch more history or verify the revisions: {}",
                stderr_message(&output)
            )));
        }
        let value = String::from_utf8(output.stdout)
            .map_err(|_| AppError::git("Git returned a non-UTF-8 merge-base"))?;
        Ok(value.trim().to_owned())
    }

    fn read_file_at(&self, commit: &str, path: &Path) -> AppResult<String> {
        let git_path = path_to_git(path)?;
        let object = format!("{commit}:{git_path}");
        let output = self.git_output_raw(["show", "--no-textconv", object.as_str()])?;
        if !output.status.success() {
            return Err(AppError::config(format!(
                "policy {git_path:?} does not exist in trusted revision {}",
                short_commit(commit)
            )));
        }
        String::from_utf8(output.stdout)
            .map_err(|_| AppError::config("policy file is not valid UTF-8"))
    }

    fn git_output<const N: usize>(&self, args: [&str; N]) -> AppResult<Output> {
        let output = self.git_output_raw(args)?;
        if output.status.success() {
            Ok(output)
        } else {
            Err(AppError::git(format!(
                "Git command failed: {}",
                stderr_message(&output)
            )))
        }
    }

    fn git_output_raw<I, S>(&self, args: I) -> AppResult<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .map_err(|error| AppError::git(format!("cannot run Git: {error}")))
    }
}

#[derive(Default)]
struct ChangeSet {
    entries: BTreeMap<String, BTreeSet<String>>,
}

impl ChangeSet {
    fn extend(&mut self, changes: Vec<(String, String)>) {
        for (path, state) in changes {
            self.entries.entry(path).or_default().insert(state);
        }
    }

    fn into_paths(self) -> Vec<ChangedPath> {
        self.entries
            .into_iter()
            .map(|(path, states)| ChangedPath {
                path,
                states: states.into_iter().collect(),
            })
            .collect()
    }
}

fn parse_name_status(bytes: &[u8], source: &str) -> AppResult<Vec<(String, String)>> {
    let fields = nul_fields(bytes)?;
    let mut changes = Vec::new();
    let mut index = 0;

    while index < fields.len() {
        let status = &fields[index];
        index += 1;
        let code = status
            .chars()
            .next()
            .ok_or_else(|| AppError::git("Git returned an empty change status"))?;

        let path = fields
            .get(index)
            .ok_or_else(|| AppError::git("Git returned a truncated change record"))?;
        index += 1;

        match code {
            'R' => {
                let destination = fields
                    .get(index)
                    .ok_or_else(|| AppError::git("Git returned a truncated rename record"))?;
                index += 1;
                changes.push((normalize_git_path(path)?, format!("{source}:renamed_from")));
                changes.push((
                    normalize_git_path(destination)?,
                    format!("{source}:renamed_to"),
                ));
            }
            'C' => {
                let destination = fields
                    .get(index)
                    .ok_or_else(|| AppError::git("Git returned a truncated copy record"))?;
                index += 1;
                changes.push((
                    normalize_git_path(destination)?,
                    format!("{source}:copied_to"),
                ));
            }
            _ => changes.push((
                normalize_git_path(path)?,
                format!("{source}:{}", status_name(code)),
            )),
        }
    }

    Ok(changes)
}

fn parse_untracked(bytes: &[u8]) -> AppResult<Vec<(String, String)>> {
    nul_fields(bytes)?
        .into_iter()
        .map(|path| Ok((normalize_git_path(&path)?, "untracked".to_owned())))
        .collect()
}

fn nul_fields(bytes: &[u8]) -> AppResult<Vec<String>> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .map(|field| {
            String::from_utf8(field.to_vec())
                .map_err(|_| AppError::git("Git returned a non-UTF-8 path"))
        })
        .collect()
}

fn normalize_git_path(path: &str) -> AppResult<String> {
    if path.contains('\\') {
        return Err(AppError::git(format!(
            "Git returned a path with a literal backslash, which is not portable: {path:?}"
        )));
    }
    if path.starts_with('/')
        || path.as_bytes().get(1) == Some(&b':')
        || path.split('/').any(|part| part == "..")
    {
        return Err(AppError::git(format!(
            "Git returned an unsafe repository path: {path:?}"
        )));
    }
    Ok(path.to_owned())
}

fn path_to_git(path: &Path) -> AppResult<String> {
    path.to_str()
        .map(|value| value.replace('\\', "/"))
        .ok_or_else(|| AppError::config("policy path must be valid UTF-8"))
}

fn status_name(code: char) -> &'static str {
    match code {
        'A' => "added",
        'D' => "deleted",
        'M' => "modified",
        'T' => "type_changed",
        'U' => "unmerged",
        'X' => "unknown",
        'B' => "broken_pair",
        _ => "changed",
    }
}

fn short_commit(commit: &str) -> &str {
    commit.get(..12).unwrap_or(commit)
}

fn stderr_message(output: &Output) -> String {
    let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if message.is_empty() {
        format!("exit status {}", output.status)
    } else {
        message
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rename_ends() {
        let parsed = parse_name_status(b"R100\0old name.rs\0new name.rs\0", "staged").unwrap();
        assert_eq!(
            parsed,
            vec![
                ("old name.rs".to_owned(), "staged:renamed_from".to_owned()),
                ("new name.rs".to_owned(), "staged:renamed_to".to_owned()),
            ]
        );
    }

    #[test]
    fn rejects_literal_backslashes_in_git_paths() {
        let error = normalize_git_path(r"src\payload.rs").unwrap_err();
        assert!(error.to_string().contains("literal backslash"));
    }
}
