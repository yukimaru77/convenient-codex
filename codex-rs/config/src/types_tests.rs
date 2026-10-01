use super::*;
use pretty_assertions::assert_eq;

#[test]
fn deserialize_skill_config_with_name_selector() {
    let cfg: SkillConfig = toml::from_str(
        r#"
            name = "github:yeet"
            enabled = false
        "#,
    )
    .expect("should deserialize skill config with name selector");

    assert_eq!(cfg.name.as_deref(), Some("github:yeet"));
    assert_eq!(cfg.path, None);
    assert!(!cfg.enabled);
}

#[test]
fn deserialize_skill_config_with_path_selector() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let skill_path = tempdir.path().join("skills").join("demo").join("SKILL.md");
    let cfg: SkillConfig = toml::from_str(&format!(
        r#"
            path = {path:?}
            enabled = false
        "#,
        path = skill_path.display().to_string(),
    ))
    .expect("should deserialize skill config with path selector");

    assert_eq!(
        cfg,
        SkillConfig {
            path: Some(
                AbsolutePathBuf::from_absolute_path(&skill_path)
                    .expect("skill path should be absolute"),
            ),
            name: None,
            enabled: false,
        }
    );
}

#[test]
fn memories_config_clamps_count_limits_to_nonzero_values() {
    let config = MemoriesConfig::from(MemoriesToml {
        max_raw_memories_for_consolidation: Some(0),
        max_rollouts_per_startup: Some(0),
        ..Default::default()
    });

    assert_eq!(
        config,
        MemoriesConfig {
            max_raw_memories_for_consolidation: 1,
            max_rollouts_per_startup: 1,
            ..MemoriesConfig::default()
        }
    );
}

#[test]
fn memories_config_clamps_rate_limit_remaining_threshold() {
    let config = MemoriesConfig::from(MemoriesToml {
        min_rate_limit_remaining_percent: Some(101),
        ..Default::default()
    });
    assert_eq!(
        config,
        MemoriesConfig {
            min_rate_limit_remaining_percent: 100,
            ..MemoriesConfig::default()
        }
    );

    let config = MemoriesConfig::from(MemoriesToml {
        min_rate_limit_remaining_percent: Some(-1),
        ..Default::default()
    });
    assert_eq!(
        config,
        MemoriesConfig {
            min_rate_limit_remaining_percent: 0,
            ..MemoriesConfig::default()
        }
    );
}

#[test]
fn memories_version_selects_pipeline_without_changing_other_defaults() {
    for (source, version) in [
        ("", MemoryVersion::V1),
        ("version = \"v2\"", MemoryVersion::V2),
    ] {
        let parsed: MemoriesToml = toml::from_str(source).expect("parse memories config");
        assert_eq!(
            MemoriesConfig::from(parsed),
            MemoriesConfig {
                version,
                ..Default::default()
            }
        );
    }
    assert!(toml::from_str::<MemoriesToml>("version = \"v3\"").is_err());
}

#[test]
fn rendering_preferences_default_individually_and_ignore_animation_switch() {
    for key in ["mermaid", "math", "tables", "lists"] {
        let tui: Tui =
            toml::from_str(&format!("animations = false\n[rendering]\n{key} = false\n")).unwrap();
        assert_eq!(
            tui.rendering,
            TuiRendering {
                mermaid: key != "mermaid",
                math: key != "math",
                tables: key != "tables",
                lists: key != "lists",
            }
        );
    }
}

#[test]
fn account_rotation_requires_accounts_dir_and_applies_defaults() {
    assert_eq!(
        AccountRotationConfig::from_toml(&AccountRotationToml::default()),
        None
    );

    let tempdir = tempfile::tempdir().expect("tempdir");
    let toml: AccountRotationToml = toml::from_str(&format!(
        "accounts_dir = {:?}\n",
        tempdir.path().display().to_string()
    ))
    .expect("should deserialize account rotation config");
    assert_eq!(
        AccountRotationConfig::from_toml(&toml),
        Some(AccountRotationConfig {
            accounts_dir: AbsolutePathBuf::from_absolute_path(tempdir.path())
                .expect("absolute path"),
            reserve_percent: DEFAULT_ACCOUNT_ROTATION_RESERVE_PERCENT,
            usage_cache_seconds: DEFAULT_ACCOUNT_ROTATION_USAGE_CACHE_SECONDS,
        })
    );

    let toml: AccountRotationToml = toml::from_str(&format!(
        "accounts_dir = {:?}\nreserve_percent = 250\nusage_cache_seconds = 5\n",
        tempdir.path().display().to_string()
    ))
    .expect("should deserialize account rotation config");
    let config = AccountRotationConfig::from_toml(&toml).expect("enabled");
    assert_eq!(
        (config.reserve_percent, config.usage_cache_seconds),
        (100, 5)
    );
}
