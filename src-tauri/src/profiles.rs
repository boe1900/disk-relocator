use serde::Deserialize;
use std::sync::OnceLock;

const PROFILES_JSON: &str = include_str!("../../specs/v1/app-profiles.json");

#[derive(Debug, Clone, Deserialize)]
pub struct ProfileSet {
    pub profiles: Vec<AppProfile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppProfile {
    pub app_id: String,
    pub display_name: String,
    pub tier: String,
    #[serde(default)]
    pub bundle_ids: Vec<String>,
    #[serde(default)]
    pub process_names: Vec<String>,
    #[serde(default)]
    pub source_paths: Vec<String>,
    #[serde(default)]
    pub target_path_template: String,
    #[serde(default)]
    pub precheck_rules: PrecheckRules,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PrecheckRules {
    #[serde(default)]
    pub allow_bootstrap_if_source_missing: bool,
    #[serde(default)]
    pub require_process_stopped: bool,
    #[serde(default)]
    pub require_full_disk_access: bool,
}

static PROFILE_CACHE: OnceLock<Result<ProfileSet, String>> = OnceLock::new();

fn load_profiles() -> Result<&'static ProfileSet, String> {
    let parsed = PROFILE_CACHE.get_or_init(|| {
        serde_json::from_str::<ProfileSet>(PROFILES_JSON)
            .map_err(|err| format!("failed to parse app-profiles.json: {err}"))
    });

    match parsed {
        Ok(profile_set) => Ok(profile_set),
        Err(err) => Err(err.clone()),
    }
}

pub fn list_profiles() -> Result<Vec<AppProfile>, String> {
    Ok(load_profiles()?.profiles.clone())
}

pub fn profile_by_id(app_id: &str) -> Result<Option<AppProfile>, String> {
    let profile_set = load_profiles()?;
    Ok(profile_set
        .profiles
        .iter()
        .find(|profile| profile.app_id == app_id)
        .cloned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn list_profiles_has_unique_app_ids() {
        let profiles = list_profiles().expect("load profiles");
        let mut ids = HashSet::new();
        for profile in profiles {
            assert!(
                ids.insert(profile.app_id.clone()),
                "duplicate app_id found: {}",
                profile.app_id
            );
        }
    }

    #[test]
    fn wechat_profile_has_expected_bundle_and_mode_flags() {
        let profile = profile_by_id("wechat-non-mas")
            .expect("load profiles")
            .expect("wechat profile should exist");
        assert_eq!(profile.tier, "experimental");
        assert!(
            profile
                .bundle_ids
                .contains(&"com.tencent.xinWeChat".to_string()),
            "wechat profile should contain bundle id com.tencent.xinWeChat"
        );
        assert!(
            !profile.precheck_rules.allow_bootstrap_if_source_missing,
            "wechat profile should not allow bootstrap when source is missing"
        );
        assert_eq!(profile.source_paths.len(), 1);
    }

    #[test]
    fn non_blocked_profiles_require_source_and_target_paths() {
        let profiles = list_profiles().expect("load profiles");
        for profile in profiles {
            if profile.tier == "blocked" {
                continue;
            }
            assert!(
                !profile.source_paths.is_empty(),
                "non-blocked profile {} should have source_paths",
                profile.app_id
            );
            assert!(
                !profile.target_path_template.trim().is_empty(),
                "non-blocked profile {} should have target_path_template",
                profile.app_id
            );
        }
    }

    #[test]
    fn profile_by_id_returns_none_for_unknown_app() {
        let unknown = profile_by_id("not-exist-app-id").expect("load profiles");
        assert!(unknown.is_none());
    }
}
