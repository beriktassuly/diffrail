use std::{fs, path::Path, process::Command};

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

const POLICY: &str = r#"version: 1
policy:
  protected:
    - ".github/**"
  shared:
    - "src/contracts/**"
tasks:
  app:
    allow:
      - "src/**"
      - ".gitignore"
    allow_shared: []
    allow_protected: []
"#;

struct TestRepo {
    directory: TempDir,
}

impl TestRepo {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        run_git(directory.path(), ["init", "-q"]);
        run_git(directory.path(), ["config", "user.name", "Test User"]);
        run_git(
            directory.path(),
            ["config", "user.email", "test@example.com"],
        );
        run_git(directory.path(), ["config", "core.autocrlf", "false"]);
        Self { directory }
    }

    fn path(&self) -> &Path {
        self.directory.path()
    }

    fn write(&self, path: &str, contents: &str) {
        let target = self.path().join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(target, contents).unwrap();
    }

    fn commit_all(&self, message: &str) -> String {
        run_git(self.path(), ["add", "--all"]);
        run_git(self.path(), ["commit", "-q", "-m", message]);
        git_stdout(self.path(), ["rev-parse", "HEAD"])
    }

    fn seed(&self) -> String {
        self.write(".diffrail.yml", POLICY);
        self.write("src/app.rs", "fn main() {}\n");
        self.write("src/contracts/api.rs", "pub struct Api;\n");
        self.write("outside.txt", "baseline\n");
        self.commit_all("seed")
    }

    fn command(&self) -> assert_cmd::Command {
        let mut command = cargo_bin_cmd!("diffrail");
        command.arg("--repo").arg(self.path());
        command
    }
}

#[test]
fn init_is_safe_and_does_not_overwrite() {
    let repo = TestRepo::new();

    repo.command().arg("init").assert().success();
    let original = fs::read(repo.path().join(".diffrail.yml")).unwrap();

    repo.command()
        .arg("init")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("already exists"));

    assert_eq!(
        original,
        fs::read(repo.path().join(".diffrail.yml")).unwrap()
    );
}

#[test]
fn local_check_allows_scoped_changes() {
    let repo = TestRepo::new();
    repo.seed();
    repo.write("src/app.rs", "fn main() { println!(\"ok\"); }\n");

    repo.command()
        .args(["check", "--task", "app"])
        .assert()
        .success()
        .stdout(predicate::str::contains("PASS app"));
}

#[test]
fn local_check_reports_untracked_outside_scope() {
    let repo = TestRepo::new();
    repo.seed();
    repo.write("notes.txt", "not assigned\n");

    repo.command()
        .args(["check", "--task", "app"])
        .assert()
        .code(1)
        .stdout(
            predicate::str::contains("outside_scope").and(predicate::str::contains("notes.txt")),
        );
}

#[test]
fn ignored_untracked_files_are_not_checked() {
    let repo = TestRepo::new();
    repo.write(".diffrail.yml", POLICY);
    repo.write(".gitignore", "ignored.log\n");
    repo.write("src/app.rs", "fn main() {}\n");
    repo.commit_all("seed");
    repo.write("ignored.log", "local output\n");

    repo.command()
        .args(["check", "--task", "app", "--quiet"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn working_copy_cannot_authorize_its_own_policy_change() {
    let repo = TestRepo::new();
    repo.seed();
    let widened = POLICY.replace("allow_protected: []", "allow_protected: [\"**\"]");
    repo.write(".diffrail.yml", &widened);

    repo.command()
        .args(["check", "--task", "app"])
        .assert()
        .code(1)
        .stdout(
            predicate::str::contains("protected_path")
                .and(predicate::str::contains(".diffrail.yml")),
        );
}

#[test]
fn shared_paths_require_explicit_shared_permission() {
    let repo = TestRepo::new();
    repo.seed();
    repo.write("src/contracts/api.rs", "pub struct ChangedApi;\n");

    repo.command()
        .args(["check", "--task", "app"])
        .assert()
        .code(1)
        .stdout(
            predicate::str::contains("shared_path")
                .and(predicate::str::contains("src/contracts/api.rs")),
        );
}

#[test]
fn staged_change_is_checked_even_when_worktree_matches_head() {
    let repo = TestRepo::new();
    repo.seed();
    repo.write("outside.txt", "staged change\n");
    run_git(repo.path(), ["add", "outside.txt"]);
    repo.write("outside.txt", "baseline\n");

    repo.command()
        .args(["check", "--task", "app"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("outside.txt"));
}

#[test]
fn base_check_rejects_committed_policy_self_widening() {
    let repo = TestRepo::new();
    let base = repo.seed();
    let widened = POLICY.replace("allow_protected: []", "allow_protected: [\"**\"]");
    repo.write(".diffrail.yml", &widened);
    repo.write(".github/workflows/release.yml", "name: release\n");
    repo.commit_all("widen policy");

    repo.command()
        .args(["check", "--task", "app", "--base", &base])
        .assert()
        .code(1)
        .stdout(
            predicate::str::contains(".diffrail.yml")
                .and(predicate::str::contains(".github/workflows/release.yml")),
        );
}

#[test]
fn base_check_uses_newer_policy_from_diverged_base() {
    let repo = TestRepo::new();
    let permissive = POLICY.replace(
        "      - \".gitignore\"",
        "      - \".gitignore\"\n      - \"outside.txt\"",
    );
    repo.write(".diffrail.yml", &permissive);
    repo.write("src/app.rs", "fn main() {}\n");
    repo.write("outside.txt", "baseline\n");
    let fork_point = repo.commit_all("permissive policy");

    repo.write("outside.txt", "feature change\n");
    let feature_head = repo.commit_all("feature change");

    run_git(repo.path(), ["checkout", "-q", "--detach", &fork_point]);
    repo.write(".diffrail.yml", POLICY);
    let trusted_base = repo.commit_all("tighten policy");

    repo.command()
        .args([
            "check",
            "--task",
            "app",
            "--base",
            &trusted_base,
            "--head",
            &feature_head,
        ])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("outside.txt"));
}

#[test]
fn rename_requires_both_paths_to_be_allowed() {
    let repo = TestRepo::new();
    repo.seed();
    run_git(repo.path(), ["mv", "outside.txt", "src/moved.txt"]);

    repo.command()
        .args(["check", "--task", "app"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("outside.txt"));
}

#[test]
fn json_report_is_structured_and_deterministic() {
    let repo = TestRepo::new();
    repo.seed();
    repo.write("z.txt", "z\n");
    repo.write("a.txt", "a\n");

    let output = repo
        .command()
        .args(["check", "--task", "app", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["ok"], false);
    assert_eq!(value["changes"][0]["path"], "a.txt");
    assert_eq!(value["changes"][1]["path"], "z.txt");
    assert_eq!(value["summary"]["violations"], 2);
}

#[test]
fn invalid_base_fails_closed() {
    let repo = TestRepo::new();
    repo.seed();

    repo.command()
        .args(["check", "--task", "app", "--base", "missing-revision"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("cannot resolve revision"));
}

fn run_git<const N: usize>(directory: &Path, args: [&str; N]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}

fn git_stdout<const N: usize>(directory: &Path, args: [&str; N]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
