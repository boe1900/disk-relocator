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
