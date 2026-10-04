# Policy reference

DiffRail uses one versioned YAML file at the repository root. The default path is `.diffrail.yml`.

## Evaluation order

Every changed path is classified in this order:

1. The policy file itself and paths matching `policy.protected`.
2. Paths matching `policy.shared`.
3. All other repository paths.

The selected task must authorize the path in the corresponding list:

| Path class | Task field |
| --- | --- |
| protected | `allow_protected` |
| shared | `allow_shared` |
| ordinary | `allow` |

A broad ordinary rule such as `**` never overrides a protected or shared rule. An empty list denies every path in that class.

Patterns are case-sensitive repository-relative globs and always use `/` separators. A trailing `/**` matches both the named path itself and everything below it. Absolute paths, parent traversal, and backslash separators are rejected.

## Policy source

`check` never reads policy from the uncommitted working copy:

- Without `--base`, it reads the policy from `HEAD` and checks staged, unstaged, and untracked changes. This is a pre-commit check; the current branch's committed `HEAD` is the policy source.
- With `--base`, it reads policy from that exact base revision, resolves its merge-base with `--head`, and checks the committed diff from the merge-base. When `--head` is the current `HEAD`, pending working-tree changes are included too.

The policy path is always treated as protected, even if it is absent from `policy.protected`. In base-aware mode, a branch therefore cannot authorize itself by editing its own rules.

## Change collection

The checker includes additions, modifications, deletions, type changes, and untracked files. Ignored untracked files are excluded. Both the source and destination of a detected rename must be authorized.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Every changed path is authorized. |
| `1` | One or more policy violations were found. |
| `2` | Arguments or policy configuration are invalid. |
| `3` | Git, filesystem, or another operational dependency failed. |

## CI boundary

The task ID is an authorization input. A protected workflow should pass a fixed task ID or derive it only from trusted configuration. Do not let pull-request code choose a broader task.

Use the immutable pull-request base SHA and fetch full history so Git can resolve a merge-base. Run on `pull_request`, not on a privileged workflow that executes untrusted code.

Because a pull request can propose changes to its own workflow, pair the check with a required status check and your repository's existing branch or ruleset protections. A path checker is not authoritative if the checked branch can silently replace the check itself.

Version 0.2 evaluates the raw head diff from the merge-base; it does not construct a prospective merge tree. If your CI platform checks a branch head instead of a generated pull-request merge result, require the branch to be current with the base before accepting the result. This avoids missing a collision where the base branch renamed a path into a protected location after the feature branch diverged.
