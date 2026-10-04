use std::{fs, path::Path};

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
