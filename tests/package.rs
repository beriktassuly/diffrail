use std::{
    fs,
    path::{Component, Path},
};

use serde_json::Value;

fn package_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn read_json(path: &str) -> Value {
    let contents = fs::read_to_string(package_root().join(path))
        .unwrap_or_else(|error| panic!("cannot read {path}: {error}"));
    serde_json::from_str(&contents).unwrap_or_else(|error| panic!("cannot parse {path}: {error}"))
}

fn required_text<'a>(value: &'a Value, field: &str, maximum: usize) -> &'a str {
    let text = value[field]
        .as_str()
        .unwrap_or_else(|| panic!("missing string field {field}"));
    assert!(!text.trim().is_empty(), "{field} must not be empty");
    assert!(
        text.chars().count() <= maximum,
        "{field} exceeds {maximum} characters"
    );
    text
}

fn local_asset_path(value: &Value, field: &str) -> std::path::PathBuf {
    let reference = required_text(value, field, 1024);
    let relative = Path::new(reference);
    assert!(!relative.is_absolute(), "{field} must be package-relative");
    assert!(
        relative
            .components()
            .all(|component| matches!(component, Component::CurDir | Component::Normal(_))),
        "{field} must not escape the package"
    );
    let asset = package_root().join(relative);
    assert!(asset.is_file(), "{field} asset is missing: {reference}");
    asset
}

fn svg_attribute<'a>(svg: &'a str, attribute: &str) -> &'a str {
    let opening = svg.split('>').next().expect("missing SVG opening tag");
    let marker = format!(" {attribute}=\"");
    opening
        .split_once(&marker)
        .unwrap_or_else(|| panic!("missing SVG {attribute}"))
        .1
        .split('"')
        .next()
        .unwrap()
}

fn assert_square_svg(asset: &Path) {
    assert_eq!(
        asset.extension().and_then(|value| value.to_str()),
        Some("svg")
    );
    let svg = fs::read_to_string(asset).unwrap();
    assert!(svg.starts_with("<svg "));
    let width: f64 = svg_attribute(&svg, "width").parse().unwrap();
    let height: f64 = svg_attribute(&svg, "height").parse().unwrap();
    assert!(width >= 48.0, "listing icon must be at least 48 pixels");
    assert_eq!(width, height, "listing icon must be square");
    let view_box: Vec<f64> = svg_attribute(&svg, "viewBox")
        .split_whitespace()
        .map(|part| part.parse().unwrap())
        .collect();
    assert_eq!(view_box.len(), 4);
    assert!(view_box[2] >= 48.0);
    assert_eq!(view_box[2], view_box[3], "SVG viewBox must be square");
    assert!(!svg.contains("<script"), "listing SVG must not run scripts");
    assert!(
        !svg.contains("<image"),
        "listing SVG must be self-contained"
    );
    assert!(
        !svg.contains("@font-face"),
        "listing SVG must not load fonts"
    );
}

#[test]
fn release_versions_are_consistent_across_packages() {
    let expected = env!("CARGO_PKG_VERSION");
    for path in [
        "plugin.json",
        ".claude-plugin/plugin.json",
        ".cursor-plugin/plugin.json",
    ] {
        let manifest = read_json(path);
        assert_eq!(manifest["name"], "diffrail", "{path}");
        assert_eq!(manifest["version"], expected, "{path}");
        assert_eq!(manifest["author"]["name"], "Beriktassuly", "{path}");
        assert_eq!(
            manifest["repository"], "https://github.com/beriktassuly/diffrail",
            "{path}"
        );
    }

    let lock = fs::read_to_string(package_root().join("Cargo.lock")).unwrap();
    let package = lock
        .split("[[package]]")
        .find(|section| section.lines().any(|line| line == "name = \"diffrail\""))
        .expect("diffrail entry missing from Cargo.lock");
    assert!(
        package
            .lines()
            .any(|line| line == format!("version = \"{expected}\""))
    );

    for catalog in [
        ".agents/plugins/marketplace.json",
        ".claude-plugin/marketplace.json",
    ] {
        let marketplace = read_json(catalog);
        for plugin in marketplace["plugins"].as_array().unwrap() {
            if let Some(version) = plugin.get("version") {
                assert_eq!(version, expected, "{catalog}");
            }
        }
    }
}

#[test]
fn openai_listing_metadata_is_complete_for_a_local_skill() {
    let manifest = read_json("plugin.json");
    let interface = &manifest["extensions"]["com.openai"]["interface"];
    assert_eq!(required_text(interface, "displayName", 30), "DiffRail");
    required_text(interface, "shortDescription", 30);
    let description = required_text(interface, "longDescription", 4000);
    assert!(
        description.contains("CLI"),
        "listing must disclose its CLI prerequisite"
    );
    assert!(
        description.contains("Git"),
        "listing must disclose its Git prerequisite"
    );
    required_text(interface, "developerName", 80);
    assert_eq!(interface["category"], "Developer Tools");
    assert_eq!(
        interface["websiteURL"],
        "https://github.com/beriktassuly/diffrail"
    );
    assert_eq!(
        interface["supportURL"],
        "https://github.com/beriktassuly/diffrail/issues"
    );
    assert_eq!(interface["brandColor"], "#335CFF");
    assert_eq!(interface["brandColorDark"], "#819AFF");
    let capabilities = interface["capabilities"].as_array().unwrap();
    assert!(!capabilities.is_empty());
    assert!(capabilities.iter().all(|capability| {
        capability
            .as_str()
            .is_some_and(|text| !text.trim().is_empty())
    }));
    let prompts = interface["defaultPrompt"].as_array().unwrap();
    assert!(!prompts.is_empty());
    for prompt in prompts {
        let text = prompt.as_str().unwrap();
        assert!(!text.trim().is_empty());
        assert!(text.chars().count() <= 128);
        assert!(!text.contains('@'), "default prompts must not use mentions");
    }
    for field in ["logo", "composerIcon"] {
        assert_square_svg(&local_asset_path(interface, field));
    }
    assert!(interface.get("screenshots").is_none());
}

#[test]
fn skill_package_has_no_automatic_hooks_or_remote_servers() {
    for path in [
        "plugin.json",
        ".claude-plugin/plugin.json",
        ".cursor-plugin/plugin.json",
    ] {
        let manifest = read_json(path);
        for field in ["hooks", "mcpServers", "mcp", "screenshots"] {
            assert!(
                manifest.get(field).is_none(),
                "unexpected {field} in {path}"
            );
        }
    }
    for path in ["hooks", "hooks.json", ".mcp.json", "mcp.json"] {
        assert!(!package_root().join(path).exists(), "unexpected {path}");
    }
    let cursor = read_json(".cursor-plugin/plugin.json");
    assert_square_svg(&local_asset_path(&cursor, "logo"));
}

#[test]
fn current_installation_docs_and_release_notes_match_the_version() {
    let version = env!("CARGO_PKG_VERSION");
    let readme = fs::read_to_string(package_root().join("README.md")).unwrap();
    assert!(readme.contains(&format!("v{version}")));
    for stale in ["--tag v0.2.0", "--ref v0.2.0", "diffrail@v0.2.0"] {
        assert!(
            !readme.contains(stale),
            "stale installation command: {stale}"
        );
    }
    let notes_path = format!("docs/releases/v{version}.md");
    let notes = fs::read_to_string(package_root().join(&notes_path))
        .unwrap_or_else(|error| panic!("cannot read {notes_path}: {error}"));
    assert!(notes.contains(&format!("DiffRail v{version}")));
    assert!(!notes.trim().is_empty());
}

#[test]
fn portable_plugin_manifest_has_expected_identity() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("plugin.json")).unwrap()).unwrap();

    assert_eq!(
        manifest["$schema"],
        "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json"
    );
    assert_eq!(manifest["name"], "diffrail");
    assert_eq!(manifest["author"]["name"], "Beriktassuly");
}

#[test]
fn skill_uses_portable_frontmatter() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let skill = fs::read_to_string(root.join("skills/diffrail/SKILL.md")).unwrap();

    assert!(skill.starts_with("---\nname: diffrail\ndescription:"));
    assert!(skill.contains("diffrail check --task <task-id>"));
}

#[test]
fn action_metadata_is_a_composite_action() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let action: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(root.join("action.yml")).unwrap()).unwrap();

    assert_eq!(action["runs"]["using"], "composite");
    assert_eq!(action["inputs"]["task"]["required"], true);
    assert_eq!(action["inputs"]["base"]["required"], true);
}

#[test]
fn marketplace_catalogs_point_to_the_root_plugin() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let openai: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join(".agents/plugins/marketplace.json")).unwrap(),
    )
    .unwrap();
    let claude: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join(".claude-plugin/marketplace.json")).unwrap(),
    )
    .unwrap();

    assert_eq!(openai["name"], "diffrail");
    assert_eq!(openai["plugins"][0]["name"], "diffrail");
    assert_eq!(openai["plugins"][0]["source"]["path"], "./");
    assert_eq!(claude["name"], "diffrail");
    assert_eq!(claude["plugins"][0]["name"], "diffrail");
    assert_eq!(claude["plugins"][0]["source"], "./");
}
