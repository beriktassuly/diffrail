# Publishing DiffRail

GitHub releases, crates.io, GitHub Marketplace, and the plugin directories are separate distribution channels. A public repository or release does not automatically create a directory listing.

## Release preparation

1. Update the package version in Cargo.toml and Cargo.lock, root plugin.json, .claude-plugin/plugin.json, and .cursor-plugin/plugin.json.
2. Update version-pinned README installation commands and add docs/releases/v<version>.md.
3. Run formatting, Clippy, tests, `cargo publish --dry-run --locked`, manifest validation, and isolated installation checks.
4. Commit the checked files as Beriktassuly, push main, and wait for CI to pass.
5. Create and push a new immutable version tag. Do not move existing release tags.
6. The Release workflow builds native binaries, runs installation smoke tests, packages the plugin, writes SHA256SUMS, and creates a draft release after every job succeeds.
7. Inspect the draft assets and notes, then publish the draft. This does not publish to crates.io or the directories below.

## crates.io

Sign in as the package owner, verify the contact email, and create a scoped publishing token. Never commit or share the token.

```sh
cargo login
cargo package --list
cargo publish --dry-run --locked
cargo publish --locked
```

Publish from the exact clean release commit. Test `cargo install diffrail --version <version> --locked` only after the package exists. Published package versions cannot be overwritten.

Official instructions: https://doc.rust-lang.org/cargo/reference/publishing.html

## GitHub Actions Marketplace

Open the release in GitHub, accept the Marketplace Developer Agreement if necessary, and select **Publish this Action to the GitHub Marketplace**. Use **Code quality** as the primary category and **Continuous integration** as the secondary category. The repository owner needs two-factor authentication. Confirm the UI validates the action name and metadata. A GitHub release created by the release workflow does not automatically opt into Marketplace.

Official instructions: https://docs.github.com/en/actions/how-tos/create-and-publish-actions/publish-in-github-marketplace

## Cursor

Test a local copy under `~/.cursor/plugins/local/diffrail`, reload Cursor, and check that its skill loads. Submit the public repository URL at https://cursor.com/marketplace/publish. The native Cursor manifest references docs/assets/diffrail-icon.svg; the portable root manifest remains available to other hosts. Cursor reviews listings and updates.

Official instructions: https://prod.cursor.com/docs/reference/plugins

## Claude

Validate locally with `claude plugin validate . --strict`. A clean local result does not guarantee directory approval.

Existing GitHub marketplace installation:

```sh
claude plugin marketplace add beriktassuly/diffrail
claude plugin install diffrail@diffrail
```

For the official directory, open https://claude.ai/directory/manage, connect the GitHub account with push access, select **Submit new > Plugin bundle**, and enter `beriktassuly/diffrail`. Leave the plugin path empty and select the intended release tag. Validate, answer data-handling questions, confirm the contact and terms, and submit for review. Publish a passing version using the portal. A tracked immutable tag must be changed to a new tag for future releases; do not move the old tag. Submission requires an eligible paid plan and role.

This directory is not the `claude-plugins-official` GitHub marketplace. Do not advertise that marketplace installation identifier without actual inclusion there.

Official instructions: https://claude.com/docs/plugins/submit

## OpenAI

Upload `diffrail-plugin-v<version>.zip` from the GitHub release at https://platform.openai.com/plugins. Choose the owning organization and project, complete individual or business verification, select the verified Developer identity, resolve automated findings, and submit for review. Publish only after approval. The displayed publisher comes from the verified identity, not merely author.name or developerName in the manifest.

Listing metadata lives in root plugin.json under extensions.com.openai.interface. The package includes the referenced square icon and at least one skill. It intentionally has no MCP configuration, lifecycle hooks, or screenshot declaration. A separate website, privacy URL, and terms URL are not all required for a skills-only upload. Currently, adding MCP to an already published skills-only plugin is not supported.

Official instructions: https://developers.openai.com/plugins/deploy/submission

## Data-handling facts for submission

- The installed CLI makes no network calls and has no DiffRail account, backend, or telemetry service.
- It reads the local Git repository and policy, and reports changed paths and violations. Those paths can themselves contain personal or confidential information.
- Initialization writes a policy file. The checking workflow does not require transmitting repository content to a DiffRail-operated service.
- Plugin hosts may process prompts, repository context, command results, and installation metadata under their own policies. Do not promise that installing a skill prevents the host from sending data to its service.
- Installation, updates, and the current source-building GitHub Action require network access.
- The plugin needs a host with filesystem and terminal access plus the separately installed CLI. Listing in a web directory does not grant access to a local repository.

Answer each portal's questions from the actual behavior and its definitions. Do not claim that directory publication, security review, or crates.io publication has happened until it has.
