use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, SystemTime};

const PROFILES_JSON: &str = include_str!("../../specs/v1/app-profiles.json");
const REMOTE_PROFILES_URL: &str =
    "https://github.com/boe1900/disk-relocator/releases/latest/download/app-profiles.json";
const PROFILES_CACHE_FILE_NAME: &str = "app-profiles-cache.json";
const PROFILES_CACHE_TTL_SECS: u64 = 24 * 60 * 60;
const PROFILES_FETCH_TIMEOUT_SECS: u64 = 5;

fn default_false() -> bool {
    false
}

fn default_true() -> bool {
    true
}

fn default_active() -> String {
    "active".to_string()
}

fn default_stable() -> String {
    "stable".to_string()
}

fn default_process_policy() -> RawProcessPolicy {
    RawProcessPolicy {
        require_process_stopped: Some(true),
        require_full_disk_access: Some(true),
    }
}

#[derive(Debug, Clone)]
pub struct ProfileSet {
    pub profiles: Vec<AppProfile>,
}

#[derive(Debug, Clone)]
pub struct AppProfile {
    pub app_id: String,
    pub display_name: String,
    pub description_i18n: BTreeMap<String, String>,
    pub migration_warning_i18n: BTreeMap<String, String>,
    pub migration_warning_countdown_seconds: u32,
    pub availability: String,
    pub blocked_reason: Option<String>,
    pub bundle_ids: Vec<String>,
    pub process_names: Vec<String>,
    pub relocation_units: Vec<RelocationUnit>,
    pub precheck_rules: PrecheckRules,
}

#[derive(Debug, Clone, Default)]
pub struct RelocationUnit {
    pub unit_id: String,
    pub display_name: String,
    pub source_path: String,
    pub target_path_template: String,
    pub default_enabled: bool,
    pub enabled: bool,
    pub risk_level: String,
    pub blocked_reason: Option<String>,
    pub allow_bootstrap_if_source_missing: bool,
    pub category: String,
}

#[derive(Debug, Clone, Default)]
pub struct PrecheckRules {
    pub allow_bootstrap_if_source_missing: bool,
    pub require_process_stopped: bool,
    pub require_full_disk_access: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RawProfileSet {
    #[serde(default)]
    pub engine_defaults: RawEngineDefaults,
    #[serde(default)]
    pub profiles: Vec<RawAppProfile>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RawEngineDefaults {
    #[serde(default)]
    pub app_level: RawAppLevelDefaults,
    #[serde(default)]
    pub unit_level: RawUnitLevelDefaults,
}

#[derive(Debug, Clone, Deserialize)]
struct RawAppLevelDefaults {
    #[serde(default = "default_active")]
    pub availability: String,
    #[serde(default = "default_process_policy")]
    pub process_policy: RawProcessPolicy,
}

impl Default for RawAppLevelDefaults {
    fn default() -> Self {
        Self {
            availability: default_active(),
            process_policy: default_process_policy(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct RawUnitLevelDefaults {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_stable")]
    pub risk_level: String,
    #[serde(default = "default_false")]
    pub allow_bootstrap_if_source_missing: bool,
}

impl Default for RawUnitLevelDefaults {
    fn default() -> Self {
        Self {
            enabled: true,
            risk_level: default_stable(),
            allow_bootstrap_if_source_missing: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct RawProcessPolicy {
    #[serde(default)]
    pub require_process_stopped: Option<bool>,
    #[serde(default)]
    pub require_full_disk_access: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RawAppProfile {
    pub app_id: String,
    pub display_name: String,
    #[serde(default)]
    pub description_i18n: BTreeMap<String, String>,
    #[serde(default)]
    pub migration_warning_i18n: BTreeMap<String, String>,
    #[serde(default)]
    pub migration_warning_countdown_seconds: Option<u32>,
    #[serde(default)]
    pub availability: String,
    #[serde(default)]
    pub blocked_reason: Option<String>,
    #[serde(default)]
    pub bundle_ids: Vec<String>,
    #[serde(default)]
    pub process_names: Vec<String>,
    #[serde(default)]
    pub units: Vec<RawRelocationUnit>,
    #[serde(default)]
    pub process_policy: Option<RawProcessPolicy>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RawRelocationUnit {
    #[serde(default)]
    pub unit_id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub source_path: String,
    #[serde(default)]
    pub target_path_template: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub default_enabled: Option<bool>,
    #[serde(default)]
    pub risk_level: Option<String>,
    #[serde(default)]
    pub blocked_reason: Option<String>,
    #[serde(default)]
    pub allow_bootstrap_if_source_missing: Option<bool>,
    #[serde(default)]
    pub category: String,
}

static PROFILE_STORE: OnceLock<Result<RwLock<ProfileSet>, String>> = OnceLock::new();

fn trim_or_empty(value: impl AsRef<str>) -> String {
    value.as_ref().trim().to_string()
}

fn contains_glob_meta(segment: &str) -> bool {
    segment.contains('*') || segment.contains('?') || segment.contains('[')
}

fn first_match_placeholder(template: &str) -> Option<String> {
    const PREFIX: &str = "{match_";
    let mut search_from = 0usize;

    while let Some(offset) = template[search_from..].find(PREFIX) {
        let begin = search_from + offset;
        let mut cursor = begin + PREFIX.len();
        let bytes = template.as_bytes();

        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
            cursor += 1;
        }

        if cursor > begin + PREFIX.len() && cursor < bytes.len() && bytes[cursor] == b'}' {
            return Some(template[begin..=cursor].to_string());
        }

        search_from = begin + PREFIX.len();
    }

    None
}

fn normalize_availability(raw: &RawAppProfile, default_availability: &str) -> String {
    let availability = trim_or_empty(&raw.availability).to_ascii_lowercase();
    if !availability.is_empty() {
        return availability;
    }
    let fallback = trim_or_empty(default_availability).to_ascii_lowercase();
    if fallback.is_empty() {
        "active".to_string()
    } else {
        fallback
    }
}

fn normalize_risk_level(raw: Option<&str>, default_risk: &str) -> String {
    let risk = raw.unwrap_or(default_risk).trim().to_ascii_lowercase();
    if risk.is_empty() {
        return "stable".to_string();
    }
    risk
}

fn trimmed_option(value: Option<String>) -> Option<String> {
    value.and_then(|v| {
        let trimmed = v.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn normalize_description_i18n(raw: BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    for (locale, description) in raw {
        let locale_key = locale.trim().to_ascii_lowercase();
        let text = description.trim().to_string();
        if locale_key.is_empty() || text.is_empty() {
            continue;
        }
        result.insert(locale_key, text);
    }
    result
}

fn normalize_units(
    raw_units: Vec<RawRelocationUnit>,
    unit_defaults: &RawUnitLevelDefaults,
) -> Vec<RelocationUnit> {
    raw_units
        .into_iter()
        .enumerate()
        .map(|(index, raw_unit)| {
            let unit_id = {
                let candidate = trim_or_empty(&raw_unit.unit_id);
                if candidate.is_empty() {
                    format!("unit-{}", index + 1)
                } else {
                    candidate
                }
            };

            let enabled = raw_unit.enabled.unwrap_or(unit_defaults.enabled);
            let default_enabled = raw_unit.default_enabled.unwrap_or(enabled) && enabled;

            let display_name = {
                let candidate = trim_or_empty(&raw_unit.display_name);
                if candidate.is_empty() {
                    unit_id.clone()
                } else {
                    candidate
                }
            };

            let source_path = trim_or_empty(&raw_unit.source_path);
            let target_path_template = trim_or_empty(&raw_unit.target_path_template);

            let risk_level =
                normalize_risk_level(raw_unit.risk_level.as_deref(), &unit_defaults.risk_level);
            let allow_bootstrap_if_source_missing = raw_unit
                .allow_bootstrap_if_source_missing
                .unwrap_or(unit_defaults.allow_bootstrap_if_source_missing);
            let category = {
                let candidate = trim_or_empty(&raw_unit.category);
                if candidate.is_empty() {
                    "app-data".to_string()
                } else {
                    candidate
                }
            };

            RelocationUnit {
                unit_id,
                display_name,
                source_path,
                target_path_template,
                default_enabled,
                enabled,
                risk_level,
                blocked_reason: trimmed_option(raw_unit.blocked_reason),
                allow_bootstrap_if_source_missing,
                category,
            }
        })
        .collect()
}

fn resolve_process_policy(
    app_defaults: &RawProcessPolicy,
    profile_policy: Option<&RawProcessPolicy>,
) -> (bool, bool) {
    let require_process_stopped = profile_policy
        .and_then(|policy| policy.require_process_stopped)
        .or(app_defaults.require_process_stopped)
        .unwrap_or(true);
    let require_full_disk_access = profile_policy
        .and_then(|policy| policy.require_full_disk_access)
        .or(app_defaults.require_full_disk_access)
        .unwrap_or(true);
    (require_process_stopped, require_full_disk_access)
}

fn normalize_profile_set(raw: RawProfileSet) -> Result<ProfileSet, String> {
    let app_defaults = raw.engine_defaults.app_level;
    let unit_defaults = raw.engine_defaults.unit_level;

    let mut profiles = Vec::new();

    for raw_profile in raw.profiles {
        let app_id = trim_or_empty(&raw_profile.app_id);
        let display_name = {
            let candidate = trim_or_empty(&raw_profile.display_name);
            if candidate.is_empty() {
                app_id.clone()
            } else {
                candidate
            }
        };
        let availability = normalize_availability(&raw_profile, &app_defaults.availability);
        let (require_process_stopped, require_full_disk_access) = resolve_process_policy(
            &app_defaults.process_policy,
            raw_profile.process_policy.as_ref(),
        );

        let relocation_units = normalize_units(raw_profile.units, &unit_defaults);

        let precheck_rules = PrecheckRules {
            allow_bootstrap_if_source_missing: relocation_units
                .iter()
                .any(|unit| unit.enabled && unit.allow_bootstrap_if_source_missing),
            require_process_stopped,
            require_full_disk_access,
        };

        profiles.push(AppProfile {
            app_id,
            display_name,
            description_i18n: normalize_description_i18n(raw_profile.description_i18n),
            migration_warning_i18n: normalize_description_i18n(raw_profile.migration_warning_i18n),
            migration_warning_countdown_seconds: raw_profile
                .migration_warning_countdown_seconds
                .unwrap_or(0),
            availability,
            blocked_reason: trimmed_option(raw_profile.blocked_reason),
            bundle_ids: raw_profile.bundle_ids,
            process_names: raw_profile.process_names,
            relocation_units,
            precheck_rules,
        });
    }

    validate_profiles(&profiles)?;
    Ok(ProfileSet { profiles })
}

fn validate_profiles(profiles: &[AppProfile]) -> Result<(), String> {
    let mut app_ids = HashSet::new();

    for profile in profiles {
        let app_id = profile.app_id.trim();
        if app_id.is_empty() {
            return Err("profile app_id cannot be empty".to_string());
        }
        if !app_ids.insert(app_id.to_string()) {
            return Err(format!("duplicate app_id found: {}", profile.app_id));
        }

        let availability = profile.availability.trim().to_ascii_lowercase();
        if !matches!(availability.as_str(), "active" | "blocked" | "deprecated") {
            return Err(format!(
                "profile {} has invalid availability: {}",
                profile.app_id, profile.availability
            ));
        }

        let mut unit_ids = HashSet::new();
        let enabled_count = profile
            .relocation_units
            .iter()
            .filter(|unit| unit.enabled)
            .count();

        if availability == "active" && enabled_count == 0 {
            return Err(format!(
                "profile {} is active but has no enabled units",
                profile.app_id
            ));
        }

        for unit in &profile.relocation_units {
            if unit.unit_id.trim().is_empty() {
                return Err(format!("profile {} has empty unit_id", profile.app_id));
            }
            if !unit_ids.insert(unit.unit_id.clone()) {
                return Err(format!(
                    "profile {} has duplicate unit_id: {}",
                    profile.app_id, unit.unit_id
                ));
            }
            if unit.source_path.trim().is_empty() {
                return Err(format!(
                    "profile {} unit {} has empty source_path",
                    profile.app_id, unit.unit_id
                ));
            }
            if unit.enabled && unit.target_path_template.trim().is_empty() {
                return Err(format!(
                    "profile {} unit {} is enabled but target_path_template is empty",
                    profile.app_id, unit.unit_id
                ));
            }

            let wildcard_count = unit
                .source_path
                .split('/')
                .filter(|segment| contains_glob_meta(segment))
                .count();
            if unit.enabled && wildcard_count == 0 {
                if let Some(placeholder) = first_match_placeholder(&unit.target_path_template) {
                    return Err(format!(
                        "profile {} unit {} source_path has no wildcard segment but target_path_template contains {}",
                        profile.app_id, unit.unit_id, placeholder
                    ));
                }
            }
            if unit.enabled && wildcard_count > 0 {
                for capture_index in 1..=wildcard_count {
                    let placeholder = format!("{{match_{capture_index}}}");
                    if !unit.target_path_template.contains(&placeholder) {
                        return Err(format!(
                            "profile {} unit {} source_path has {} wildcard segment(s) but target_path_template misses {}",
                            profile.app_id, unit.unit_id, wildcard_count, placeholder
                        ));
                    }
                }
            }

            let risk = unit.risk_level.trim().to_ascii_lowercase();
            if !matches!(risk.as_str(), "stable" | "cautious" | "high") {
                return Err(format!(
                    "profile {} unit {} has invalid risk_level: {}",
                    profile.app_id, unit.unit_id, unit.risk_level
                ));
            }
        }
    }

    Ok(())
}

fn parse_profile_set(payload: &str, source: &str) -> Result<ProfileSet, String> {
    serde_json::from_str::<RawProfileSet>(payload)
        .map_err(|err| format!("failed to parse {source}: {err}"))
        .and_then(normalize_profile_set)
}

fn profiles_cache_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(PROFILES_CACHE_FILE_NAME)
}

fn cache_is_fresh(cache_path: &Path, cache_ttl: Duration) -> bool {
    let metadata = match fs::metadata(cache_path) {
        Ok(value) => value,
        Err(_) => return false,
    };
    let modified = match metadata.modified() {
        Ok(value) => value,
        Err(_) => return false,
    };
    match SystemTime::now().duration_since(modified) {
        Ok(age) => age <= cache_ttl,
        Err(_) => true,
    }
}

fn read_cached_profile_set(
    app_data_dir: &Path,
    cache_ttl: Duration,
    require_fresh: bool,
) -> Option<Result<ProfileSet, String>> {
    let cache_path = profiles_cache_path(app_data_dir);
    if require_fresh && !cache_is_fresh(&cache_path, cache_ttl) {
        return None;
    }
    let payload = match fs::read_to_string(&cache_path) {
        Ok(value) => value,
        Err(err) => return Some(Err(format!("failed to read cache {cache_path:?}: {err}"))),
    };
    Some(parse_profile_set(&payload, "cached app-profiles.json"))
}

fn fetch_remote_profile_payload() -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(PROFILES_FETCH_TIMEOUT_SECS))
        .timeout(Duration::from_secs(PROFILES_FETCH_TIMEOUT_SECS))
        .build()
        .map_err(|err| format!("failed to build profile fetch client: {err}"))?;
    let response = client
        .get(REMOTE_PROFILES_URL)
        .header(
            reqwest::header::USER_AGENT,
            format!("disk-relocator/{}", env!("CARGO_PKG_VERSION")),
        )
        .send()
        .map_err(|err| format!("failed to fetch remote profile: {err}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "failed to fetch remote profile: http {}",
            response.status()
        ));
    }

    response
        .text()
        .map_err(|err| format!("failed to decode remote profile payload: {err}"))
}

fn write_profile_cache(app_data_dir: &Path, payload: &str) -> Result<(), String> {
    let cache_path = profiles_cache_path(app_data_dir);
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create cache directory {parent:?}: {err}"))?;
    }
    fs::write(&cache_path, payload)
        .map_err(|err| format!("failed to write profile cache {cache_path:?}: {err}"))
}

fn load_profile_set_with_remote_fetch<F>(
    app_data_dir: &Path,
    cache_ttl: Duration,
    remote_fetch: F,
) -> Result<ProfileSet, String>
where
    F: FnOnce() -> Result<String, String>,
{
    if let Some(cached) = read_cached_profile_set(app_data_dir, cache_ttl, true) {
        match cached {
            Ok(set) => return Ok(set),
            Err(err) => eprintln!("[profiles] ignore invalid fresh cache: {err}"),
        }
    }

    match remote_fetch() {
        Ok(payload) => match parse_profile_set(&payload, "remote app-profiles.json") {
            Ok(set) => {
                if let Err(err) = write_profile_cache(app_data_dir, &payload) {
                    eprintln!("[profiles] write cache skipped: {err}");
                }
                return Ok(set);
            }
            Err(err) => eprintln!("[profiles] ignore invalid remote profile: {err}"),
        },
        Err(err) => eprintln!("[profiles] remote profile fetch failed: {err}"),
    }

    if let Some(cached) = read_cached_profile_set(app_data_dir, cache_ttl, false) {
        match cached {
            Ok(set) => return Ok(set),
            Err(err) => eprintln!("[profiles] ignore invalid stale cache: {err}"),
        }
    }

    parse_profile_set(PROFILES_JSON, "embedded app-profiles.json")
}

fn load_profile_set_for_app(app_data_dir: &Path) -> Result<ProfileSet, String> {
    load_profile_set_with_remote_fetch(
        app_data_dir,
        Duration::from_secs(PROFILES_CACHE_TTL_SECS),
        fetch_remote_profile_payload,
    )
}

fn profile_store(app_data_dir: Option<&Path>) -> Result<&'static RwLock<ProfileSet>, String> {
    let parsed = PROFILE_STORE.get_or_init(|| {
        let profile_set = match app_data_dir {
            Some(dir) => load_profile_set_for_app(dir),
            None => parse_profile_set(PROFILES_JSON, "embedded app-profiles.json"),
        }?;
        Ok(RwLock::new(profile_set))
    });

    match parsed {
        Ok(store) => Ok(store),
        Err(err) => Err(err.clone()),
    }
}

pub fn initialize_profile_store(app_data_dir: &Path) -> Result<(), String> {
    let _ = profile_store(Some(app_data_dir))?;
    Ok(())
}

pub fn list_profiles() -> Result<Vec<AppProfile>, String> {
    let store = profile_store(None)?;
    let guard = store
        .read()
        .map_err(|_| "profile store poisoned while listing profiles".to_string())?;
    Ok(guard.profiles.clone())
}

pub fn profile_by_id(app_id: &str) -> Result<Option<AppProfile>, String> {
    let store = profile_store(None)?;
    let guard = store
        .read()
        .map_err(|_| "profile store poisoned while looking up profile".to_string())?;
    Ok(guard
        .profiles
        .iter()
        .find(|profile| profile.app_id == app_id)
        .cloned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::thread;
    use tempfile::tempdir;

    fn profile_payload_for(app_id: &str) -> String {
        format!(
            r#"{{
  "profiles": [
    {{
      "app_id": "{app_id}",
      "display_name": "{app_id}",
      "units": [
        {{
          "unit_id": "main",
          "source_path": "~/Library/{app_id}",
          "target_path_template": "{{target_root}}/AppData/{app_id}"
        }}
      ]
    }}
  ]
}}"#
        )
    }

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
    fn load_profile_set_uses_fresh_cache_without_remote_fetch() {
        let dir = tempdir().expect("create temp dir");
        let app_data_dir = dir.path();
        let payload = profile_payload_for("cache-app");
        fs::write(profiles_cache_path(app_data_dir), payload).expect("write cache");

        let remote_called = Cell::new(false);
        let set = load_profile_set_with_remote_fetch(
            app_data_dir,
            Duration::from_secs(PROFILES_CACHE_TTL_SECS),
            || {
                remote_called.set(true);
                Err("should not fetch".to_string())
            },
        )
        .expect("load profile set");

        assert!(
            !remote_called.get(),
            "fresh cache should bypass remote fetch"
        );
        assert!(
            set.profiles
                .iter()
                .any(|profile| profile.app_id == "cache-app"),
            "cache payload should be used"
        );
    }

    #[test]
    fn load_profile_set_uses_remote_and_refreshes_cache_when_cache_stale() {
        let dir = tempdir().expect("create temp dir");
        let app_data_dir = dir.path();
        fs::write(
            profiles_cache_path(app_data_dir),
            profile_payload_for("stale-app"),
        )
        .expect("write stale cache");
        thread::sleep(Duration::from_millis(5));

        let set = load_profile_set_with_remote_fetch(app_data_dir, Duration::from_secs(0), || {
            Ok(profile_payload_for("remote-app"))
        })
        .expect("load profile set");

        assert!(
            set.profiles
                .iter()
                .any(|profile| profile.app_id == "remote-app"),
            "remote payload should be used when cache is stale"
        );

        let refreshed_cache =
            fs::read_to_string(profiles_cache_path(app_data_dir)).expect("read refreshed cache");
        assert!(
            refreshed_cache.contains("\"remote-app\""),
            "remote payload should refresh cache contents"
        );
    }

    #[test]
    fn load_profile_set_falls_back_to_stale_cache_when_remote_fetch_fails() {
        let dir = tempdir().expect("create temp dir");
        let app_data_dir = dir.path();
        fs::write(
            profiles_cache_path(app_data_dir),
            profile_payload_for("stale-app"),
        )
        .expect("write stale cache");
        thread::sleep(Duration::from_millis(5));

        let set = load_profile_set_with_remote_fetch(app_data_dir, Duration::from_secs(0), || {
            Err("offline".to_string())
        })
        .expect("load profile set");

        assert!(
            set.profiles
                .iter()
                .any(|profile| profile.app_id == "stale-app"),
            "stale cache should be used when remote fetch fails"
        );
    }

    #[test]
    fn wechat_profile_has_expected_bundle_and_mode_flags() {
        let profile = profile_by_id("wechat-non-mas")
            .expect("load profiles")
            .expect("wechat profile should exist");
        assert_eq!(profile.availability, "active");
        assert!(
            profile
                .bundle_ids
                .contains(&"com.tencent.xinWeChat".to_string()),
            "wechat profile should contain bundle id com.tencent.xinWeChat"
        );
        assert!(
            !profile.precheck_rules.allow_bootstrap_if_source_missing,
            "wechat profile should not allow bootstrap for xwechat_files unit when source is missing"
        );
        let unit = profile
            .relocation_units
            .iter()
            .find(|unit| unit.unit_id == "wechat-core-xwechat-files" && unit.enabled)
            .expect("wechat profile should contain enabled wechat-core-xwechat-files unit");
        assert_eq!(
            unit.source_path,
            "~/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files",
            "wechat unit source path should point to xwechat_files root"
        );
        assert_eq!(
            unit.target_path_template, "{target_root}/AppData/WeChat/xwechat_files",
            "wechat unit target path template should point to xwechat_files root"
        );
        assert_eq!(unit.risk_level, "high");
        assert!(
            !profile.migration_warning_i18n.is_empty(),
            "wechat profile should include migration warning text"
        );
        assert_eq!(profile.migration_warning_countdown_seconds, 3);
    }

    #[test]
    fn dingtalk_profile_has_expected_bundle_process_and_unit_paths() {
        let profile = profile_by_id("dingtalk")
            .expect("load profiles")
            .expect("dingtalk profile should exist");
        assert_eq!(profile.availability, "active");
        assert!(
            profile
                .bundle_ids
                .contains(&"com.alibaba.DingTalkMac".to_string()),
            "dingtalk profile should contain bundle id com.alibaba.DingTalkMac"
        );
        assert!(
            profile.process_names.contains(&"DingTalk".to_string()),
            "dingtalk profile should contain process name DingTalk"
        );

        let expected_units = vec![
            (
                "dingtalk-web-image",
                "media",
                "~/Library/Application Support/DingTalkMac/*/ImageFiles",
                "{target_root}/AppData/DingTalk/{match_1}/ImageFiles",
            ),
            (
                "dingtalk-web-video",
                "media",
                "~/Library/Application Support/DingTalkMac/*/VideoFiles",
                "{target_root}/AppData/DingTalk/{match_1}/VideoFiles",
            ),
            (
                "dingtalk-web-audio",
                "media",
                "~/Library/Application Support/DingTalkMac/*/AudioFiles",
                "{target_root}/AppData/DingTalk/{match_1}/AudioFiles",
            ),
            (
                "dingtalk-web-emotion",
                "media",
                "~/Library/Application Support/DingTalkMac/*/GifEmotionFiles",
                "{target_root}/AppData/DingTalk/{match_1}/GifEmotionFiles",
            ),
            (
                "dingtalk-web-eapp-download",
                "cache",
                "~/Library/Application Support/DingTalkMac/*/EAppFiles/download",
                "{target_root}/AppData/DingTalk/{match_1}/EAppFiles/download",
            ),
            (
                "dingtalk-web-eapp-unziped",
                "cache",
                "~/Library/Application Support/DingTalkMac/*/EAppFiles/unziped",
                "{target_root}/AppData/DingTalk/{match_1}/EAppFiles/unziped",
            ),
        ];

        assert_eq!(
            profile.relocation_units.len(),
            expected_units.len(),
            "dingtalk profile should include all expected migration units"
        );

        for (unit_id, category, source_path, target_path_template) in expected_units {
            let unit = profile
                .relocation_units
                .iter()
                .find(|unit| unit.unit_id == unit_id && unit.enabled)
                .unwrap_or_else(|| panic!("dingtalk profile should contain enabled {unit_id}"));

            assert_eq!(
                unit.category, category,
                "{unit_id} unit category should match profile spec"
            );
            assert_eq!(
                unit.source_path, source_path,
                "{unit_id} unit source path should match profile spec"
            );
            assert_eq!(
                unit.target_path_template, target_path_template,
                "{unit_id} unit target path template should match profile spec"
            );
            assert!(
                !unit.allow_bootstrap_if_source_missing,
                "{unit_id} unit should not allow bootstrap when source is missing"
            );
        }

        assert!(
            !profile.precheck_rules.allow_bootstrap_if_source_missing,
            "dingtalk profile precheck should not allow bootstrap when source is missing"
        );
    }

    #[test]
    fn qq_profile_has_expected_bundle_process_and_unit_paths() {
        let profile = profile_by_id("qq-nt")
            .expect("load profiles")
            .expect("qq profile should exist");
        assert_eq!(profile.availability, "active");
        assert!(
            profile.bundle_ids.contains(&"com.tencent.qq".to_string()),
            "qq profile should contain bundle id com.tencent.qq"
        );
        assert!(
            profile.process_names.contains(&"QQ".to_string()),
            "qq profile should contain process name QQ"
        );
        assert!(
            !profile.migration_warning_i18n.is_empty(),
            "qq profile should include migration warning text"
        );
        assert_eq!(
            profile.migration_warning_countdown_seconds, 3,
            "qq profile should configure 3-second warning countdown"
        );

        let unit = profile
            .relocation_units
            .iter()
            .find(|unit| unit.unit_id == "qq-root" && unit.enabled)
            .expect("qq profile should contain enabled qq-root unit");

        assert_eq!(
            unit.category, "root_entity",
            "qq-root unit category should match profile spec"
        );
        assert_eq!(
            unit.risk_level, "high",
            "qq-root unit risk level should be high"
        );
        assert_eq!(
            unit.source_path,
            "~/Library/Containers/com.tencent.qq/Data/Library/Application Support/QQ/nt_qq_*",
            "qq-root unit source path should match profile spec"
        );
        assert_eq!(
            unit.target_path_template, "{target_root}/AppData/QQ_NT/{match_1}",
            "qq-root unit target path template should match profile spec"
        );
        assert!(
            !unit.allow_bootstrap_if_source_missing,
            "qq-root unit should not allow bootstrap when source is missing"
        );
    }

    #[test]
    fn active_profiles_require_enabled_units_and_valid_paths() {
        let profiles = list_profiles().expect("load profiles");
        for profile in profiles {
            if profile.availability != "active" {
                continue;
            }

            assert!(
                profile.relocation_units.iter().any(|unit| unit.enabled),
                "active profile {} should have at least one enabled unit",
                profile.app_id
            );

            for unit in &profile.relocation_units {
                assert!(
                    !unit.unit_id.trim().is_empty(),
                    "profile {} has relocation unit with empty unit_id",
                    profile.app_id
                );
                assert!(
                    !unit.source_path.trim().is_empty(),
                    "profile {} unit {} should have source_path",
                    profile.app_id,
                    unit.unit_id
                );
                if unit.enabled {
                    assert!(
                        !unit.target_path_template.trim().is_empty(),
                        "profile {} unit {} should have target_path_template when enabled",
                        profile.app_id,
                        unit.unit_id
                    );
                }
            }
        }
    }

    #[test]
    fn profile_by_id_returns_none_for_unknown_app() {
        let unknown = profile_by_id("not-exist-app-id").expect("load profiles");
        assert!(unknown.is_none());
    }

    #[test]
    fn wildcard_source_requires_all_match_placeholders_in_target_template() {
        let payload = r#"
        {
          "profiles": [
            {
              "app_id": "demo",
              "display_name": "Demo",
              "units": [
                {
                  "unit_id": "demo-media",
                  "source_path": "~/Library/Demo/*/msg/*",
                  "target_path_template": "{target_root}/AppData/Demo/{match_1}/msg"
                }
              ]
            }
          ]
        }
        "#;

        let err = parse_profile_set(payload, "test profile payload")
            .expect_err("missing wildcard placeholders should be rejected");
        assert!(
            err.contains("target_path_template misses {match_2}"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn wildcard_source_accepts_target_template_with_all_match_placeholders() {
        let payload = r#"
        {
          "profiles": [
            {
              "app_id": "demo",
              "display_name": "Demo",
              "units": [
                {
                  "unit_id": "demo-media",
                  "source_path": "~/Library/Demo/*/msg/*",
                  "target_path_template": "{target_root}/AppData/Demo/{match_1}/msg/{match_2}"
                }
              ]
            }
          ]
        }
        "#;

        let parsed = parse_profile_set(payload, "test profile payload")
            .expect("all wildcard placeholders should pass validation");
        let profile = parsed
            .profiles
            .iter()
            .find(|profile| profile.app_id == "demo")
            .expect("demo profile should exist");
        assert_eq!(profile.relocation_units.len(), 1);
        assert_eq!(
            profile.relocation_units[0].target_path_template,
            "{target_root}/AppData/Demo/{match_1}/msg/{match_2}"
        );
    }

    #[test]
    fn segment_glob_source_requires_match_placeholder_in_target_template() {
        let payload = r#"
        {
          "profiles": [
            {
              "app_id": "qq",
              "display_name": "QQ",
              "units": [
                {
                  "unit_id": "qq-msg-all",
                  "source_path": "~/Library/QQ/nt_qq_*/msg",
                  "target_path_template": "{target_root}/AppData/QQ/msg"
                }
              ]
            }
          ]
        }
        "#;

        let err = parse_profile_set(payload, "test profile payload")
            .expect_err("segment glob without match placeholder should be rejected");
        assert!(
            err.contains("target_path_template misses {match_1}"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn non_wildcard_source_rejects_match_placeholder_in_target_template() {
        let payload = r#"
        {
          "profiles": [
            {
              "app_id": "demo",
              "display_name": "Demo",
              "units": [
                {
                  "unit_id": "demo-media",
                  "source_path": "~/Library/Demo/msg",
                  "target_path_template": "{target_root}/AppData/Demo/{match_1}/msg"
                }
              ]
            }
          ]
        }
        "#;

        let err = parse_profile_set(payload, "test profile payload")
            .expect_err("non-wildcard source should reject match placeholders");
        assert!(
            err.contains("source_path has no wildcard segment"),
            "unexpected error: {err}"
        );
        assert!(err.contains("{match_1}"), "unexpected error: {err}");
    }

    #[test]
    fn non_wildcard_source_accepts_template_without_match_placeholder() {
        let payload = r#"
        {
          "profiles": [
            {
              "app_id": "demo",
              "display_name": "Demo",
              "units": [
                {
                  "unit_id": "demo-media",
                  "source_path": "~/Library/Demo/msg",
                  "target_path_template": "{target_root}/AppData/Demo/msg"
                }
              ]
            }
          ]
        }
        "#;

        let parsed = parse_profile_set(payload, "test profile payload")
            .expect("non-wildcard source without match placeholder should pass validation");
        let profile = parsed
            .profiles
            .iter()
            .find(|profile| profile.app_id == "demo")
            .expect("demo profile should exist");
        assert_eq!(profile.relocation_units.len(), 1);
        assert_eq!(
            profile.relocation_units[0].target_path_template,
            "{target_root}/AppData/Demo/msg"
        );
    }
}
