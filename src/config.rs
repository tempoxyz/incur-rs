//! Configuration waterfall using figment2.
//!
//! Provides Foundry-style config resolution where options are resolved
//! with this precedence (highest wins):
//!
//! 1. CLI arguments (handled by the parser)
//! 2. Environment variables (`PREFIX_*` or explicit `Opt::env()`)
//! 3. TOML config file (discovered by walking up from cwd)
//! 4. Code defaults (`Opt::default_value`)

use std::collections::HashMap;
use std::path::PathBuf;

use figment::Figment;
use figment::providers::{Format, Serialized, Toml};
use serde::{Deserialize, Serialize};

use crate::parser::{Opt, OptType, Value};

/// Walks up from the current directory looking for `file_name`.
fn discover_file(file_name: &str) -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join(file_name);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Converts `SCREAMING_SNAKE_CASE` to `camelCase`.
/// E.g., `"TIMEOUT"` → `"timeout"`, `"SAVE_DEV"` → `"saveDev"`.
fn env_key_to_option_name(key: &str) -> String {
    let lower = key.to_lowercase();
    let parts: Vec<&str> = lower.split('_').collect();
    if parts.len() == 1 {
        return parts[0].to_string();
    }
    let mut result = parts[0].to_string();
    for part in &parts[1..] {
        if let Some(first) = part.chars().next() {
            result.push(first.to_uppercase().next().unwrap());
            result.push_str(&part[first.len_utf8()..]);
        }
    }
    result
}

/// Builds a [`Figment`] with the config waterfall for the given options.
///
/// The figment merges providers in this order (last wins):
/// 1. Code defaults from `Opt::default_value`
/// 2. TOML config file (if `config_file` is set and discovered)
/// 3. Prefixed env vars (if `env_prefix` is set)
/// 4. Explicit env vars from `Opt::env` (per-option)
///
/// CLI arguments are NOT included here — they override during parsing.
pub fn build_figment(opts: &[Opt], config_file: Option<&str>, env_prefix: Option<&str>) -> Figment {
    // Layer 1: Code defaults
    let mut defaults = HashMap::new();
    for opt in opts {
        if let Some(ref default) = opt.default {
            defaults.insert(opt.name.clone(), default.clone());
        }
    }
    let mut figment = Figment::new().merge(Serialized::defaults(&defaults));

    // Layer 2: TOML config file
    if let Some(file_name) = config_file
        && let Some(path) = discover_file(file_name)
    {
        figment = figment.merge(Toml::file(path));
    }

    // Layer 3: Prefixed env vars (e.g., MYAPP_TIMEOUT → timeout)
    // Figment's Env provider strips the prefix and lowercases keys.
    // We map SCREAMING_SNAKE env keys to their option names so they
    // match the code-default keys used in layer 1.
    if let Some(prefix) = env_prefix {
        // Read env vars manually and inject via Serialized so we control key names.
        let prefix_underscore = format!("{prefix}_");
        for (key, val) in std::env::vars() {
            if let Some(rest) = key.strip_prefix(&prefix_underscore)
                && !rest.is_empty()
            {
                let option_name = env_key_to_option_name(rest);
                figment = figment.merge(Serialized::default(&option_name, &val));
            }
        }
    }

    // Layer 4: Explicit env vars per-option (e.g., Opt::env("MY_API_KEY"))
    for opt in opts {
        if let Some(ref env_var) = opt.env
            && let Ok(val) = std::env::var(env_var)
        {
            figment = figment.merge(Serialized::default(&opt.name, &val));
        }
    }

    figment
}

/// Builds effective options with config waterfall defaults applied.
///
/// Uses figment2 to merge code defaults → TOML → env vars, then
/// injects the resolved values as `opt.default` so that CLI args
/// (parsed later) naturally override them.
pub fn apply_waterfall(
    opts: &[Opt],
    config_file: Option<&str>,
    env_prefix: Option<&str>,
) -> Vec<Opt> {
    let figment = build_figment(opts, config_file, env_prefix);

    // Extract as a flat string map — figment handles the merge precedence
    let resolved: HashMap<String, FigmentValue> = figment.extract().unwrap_or_default();

    opts.iter()
        .map(|opt| {
            let mut opt = opt.clone();
            if let Some(val) = resolved.get(&opt.name) {
                opt.default = Some(val.to_string_value());
            }
            // Also check kebab-case and snake_case aliases in the resolved map
            if !resolved.contains_key(&opt.name) {
                let kebab = crate::parser::to_kebab(&opt.name);
                if let Some(val) = resolved.get(&kebab) {
                    opt.default = Some(val.to_string_value());
                } else {
                    let snake = kebab.replace('-', "_");
                    if let Some(val) = resolved.get(&snake) {
                        opt.default = Some(val.to_string_value());
                    }
                }
            }
            opt
        })
        .collect()
}

/// A wrapper for figment-extracted values that handles multiple types.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
enum FigmentValue {
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
}

impl FigmentValue {
    fn to_string_value(&self) -> String {
        match self {
            Self::Bool(b) => b.to_string(),
            Self::Integer(n) => n.to_string(),
            Self::Float(n) => n.to_string(),
            Self::String(s) => s.clone(),
        }
    }
}

/// Coerces a string value to the appropriate incur `Value` type.
fn coerce(value: &str, opt_type: OptType) -> Value {
    match opt_type {
        OptType::Bool => {
            let v = value.trim().to_ascii_lowercase();
            Value::Bool(v == "true" || v == "1" || v == "yes" || v == "on")
        }
        OptType::Number => Value::Number(value.parse().unwrap_or(0.0)),
        OptType::Array => Value::Array(vec![value.to_string()]),
        OptType::String => Value::String(value.to_string()),
    }
}

/// Merges config sources with waterfall precedence.
/// Returns the final resolved values for options not set via CLI.
pub fn merge_resolved(
    opts: &[Opt],
    config_file: Option<&str>,
    env_prefix: Option<&str>,
) -> HashMap<String, Value> {
    let figment = build_figment(opts, config_file, env_prefix);
    let resolved: HashMap<String, FigmentValue> = figment.extract().unwrap_or_default();

    let mut result = HashMap::new();
    for opt in opts {
        if let Some(val) = resolved.get(&opt.name) {
            result.insert(
                opt.name.clone(),
                coerce(&val.to_string_value(), opt.opt_type),
            );
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_key_to_option_name_simple() {
        assert_eq!(env_key_to_option_name("TIMEOUT"), "timeout");
        assert_eq!(env_key_to_option_name("SAVE_DEV"), "saveDev");
        assert_eq!(env_key_to_option_name("MAX_RETRY_COUNT"), "maxRetryCount");
    }

    #[test]
    fn build_figment_code_defaults() {
        let opts = vec![
            Opt::new("timeout").number().default_value("10"),
            Opt::new("name").default_value("default"),
        ];
        let figment = build_figment(&opts, None, None);
        let resolved: HashMap<String, String> = figment.extract().unwrap();
        assert_eq!(resolved.get("timeout").unwrap(), "10");
        assert_eq!(resolved.get("name").unwrap(), "default");
    }

    #[test]
    fn build_figment_toml_overrides_defaults() {
        figment::Jail::expect_with(|jail| {
            jail.create_file("test.toml", "timeout = 60\n")?;
            let opts = vec![Opt::new("timeout").number().default_value("10")];
            let figment = build_figment(&opts, Some("test.toml"), None);
            let resolved: HashMap<String, FigmentValue> = figment.extract()?;
            assert_eq!(resolved.get("timeout").unwrap().to_string_value(), "60");
            Ok(())
        });
    }

    #[test]
    fn build_figment_env_overrides_toml() {
        figment::Jail::expect_with(|jail| {
            jail.create_file("test.toml", "timeout = 30\n")?;
            jail.set_env("MYAPP_TIMEOUT", "120");
            let opts = vec![Opt::new("timeout").number().default_value("10")];
            let figment = build_figment(&opts, Some("test.toml"), Some("MYAPP"));
            let resolved: HashMap<String, FigmentValue> = figment.extract()?;
            assert_eq!(resolved.get("timeout").unwrap().to_string_value(), "120");
            Ok(())
        });
    }

    #[test]
    fn build_figment_explicit_env_overrides_prefix() {
        figment::Jail::expect_with(|jail| {
            jail.set_env("MYAPP_TIMEOUT", "60");
            jail.set_env("MY_CUSTOM_VAR", "999");
            let opts = vec![Opt::new("timeout").number().env("MY_CUSTOM_VAR")];
            let figment = build_figment(&opts, None, Some("MYAPP"));
            let resolved: HashMap<String, FigmentValue> = figment.extract()?;
            // Explicit env var wins over prefix-derived
            assert_eq!(resolved.get("timeout").unwrap().to_string_value(), "999");
            Ok(())
        });
    }

    #[test]
    fn apply_waterfall_layers() {
        figment::Jail::expect_with(|jail| {
            jail.create_file("test.toml", "timeout = 30\n")?;
            jail.set_env("MYAPP_TIMEOUT", "60");
            let opts = vec![
                Opt::new("timeout").number().default_value("10"),
                Opt::new("name").default_value("default"),
            ];
            let effective = apply_waterfall(&opts, Some("test.toml"), Some("MYAPP"));
            // env wins for timeout
            assert_eq!(effective[0].default.as_deref(), Some("60"));
            // code default preserved for name
            assert_eq!(effective[1].default.as_deref(), Some("default"));
            Ok(())
        });
    }

    #[test]
    fn apply_waterfall_toml_overrides_code_default() {
        figment::Jail::expect_with(|jail| {
            jail.create_file("test.toml", "name = \"from-toml\"\n")?;
            let opts = vec![Opt::new("name").default_value("code-default")];
            let effective = apply_waterfall(&opts, Some("test.toml"), None);
            assert_eq!(effective[0].default.as_deref(), Some("from-toml"));
            Ok(())
        });
    }

    #[test]
    fn apply_waterfall_explicit_env() {
        figment::Jail::expect_with(|jail| {
            jail.set_env("MY_CUSTOM_VAR", "from-env");
            let opts = vec![Opt::new("timeout").env("MY_CUSTOM_VAR")];
            let effective = apply_waterfall(&opts, None, None);
            assert_eq!(effective[0].default.as_deref(), Some("from-env"));
            Ok(())
        });
    }

    #[test]
    fn merge_resolved_waterfall() {
        figment::Jail::expect_with(|jail| {
            jail.create_file("test.toml", "timeout = 30\nname = \"from-toml\"\n")?;
            jail.set_env("MYAPP_TIMEOUT", "60");
            let opts = vec![
                Opt::new("timeout").number().default_value("10"),
                Opt::new("name"),
            ];
            let merged = merge_resolved(&opts, Some("test.toml"), Some("MYAPP"));
            assert_eq!(merged.get("timeout").unwrap().as_f64(), 60.0);
            assert_eq!(merged.get("name").unwrap().as_str(), "from-toml");
            Ok(())
        });
    }

    #[test]
    fn toml_supports_snake_case_keys() {
        figment::Jail::expect_with(|jail| {
            jail.create_file("test.toml", "save_dev = true\n")?;
            let opts = vec![Opt::new("save_dev").boolean()];
            let effective = apply_waterfall(&opts, Some("test.toml"), None);
            assert_eq!(effective[0].default.as_deref(), Some("true"));
            Ok(())
        });
    }

    #[test]
    fn bool_coercion_case_insensitive() {
        let merged = coerce("TRUE", OptType::Bool);
        assert!(merged.as_bool());
        let merged = coerce("Yes", OptType::Bool);
        assert!(merged.as_bool());
        let merged = coerce("ON", OptType::Bool);
        assert!(merged.as_bool());
        let merged = coerce("0", OptType::Bool);
        assert!(!merged.as_bool());
    }

    #[test]
    fn env_camel_case_mapping() {
        figment::Jail::expect_with(|jail| {
            jail.set_env("MYAPP_SAVE_DEV", "true");
            let opts = vec![Opt::new("saveDev").boolean()];
            let figment = build_figment(&opts, None, Some("MYAPP"));
            let resolved: HashMap<String, FigmentValue> = figment.extract()?;
            assert_eq!(resolved.get("saveDev").unwrap().to_string_value(), "true");
            Ok(())
        });
    }
}
