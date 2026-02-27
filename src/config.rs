use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::parser::{Opt, OptType, Value};

/// Configuration loaded from a TOML file or environment variables.
#[derive(Debug, Default)]
pub struct ConfigValues {
    pub values: HashMap<String, String>,
}

/// Discovers a TOML config file by walking up from the current directory.
/// Returns the parsed key-value pairs (top-level only).
pub fn load_toml(file_name: &str) -> ConfigValues {
    let path = match discover_file(file_name) {
        Some(p) => p,
        None => return ConfigValues::default(),
    };
    load_toml_from_path(&path)
}

/// Loads a TOML config file from a specific path.
pub fn load_toml_from_path(path: &Path) -> ConfigValues {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return ConfigValues::default(),
    };
    let table: toml::Table = match content.parse() {
        Ok(t) => t,
        Err(_) => return ConfigValues::default(),
    };

    let mut values = HashMap::new();
    for (key, val) in &table {
        let s = match val {
            toml::Value::String(s) => s.clone(),
            toml::Value::Integer(n) => n.to_string(),
            toml::Value::Float(n) => n.to_string(),
            toml::Value::Boolean(b) => b.to_string(),
            _ => continue, // skip nested tables/arrays for now
        };
        values.insert(key.clone(), s);
    }

    ConfigValues { values }
}

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

/// Reads environment variables matching the given prefix.
/// E.g., prefix `"MYAPP"` reads `MYAPP_TIMEOUT`, `MYAPP_VERBOSE`, etc.
/// Keys are converted from `SCREAMING_SNAKE_CASE` to the option name form.
pub fn load_env(prefix: &str) -> ConfigValues {
    let prefix_underscore = format!("{prefix}_");
    let mut values = HashMap::new();

    for (key, val) in std::env::vars() {
        if let Some(rest) = key.strip_prefix(&prefix_underscore) {
            if rest.is_empty() {
                continue;
            }
            let option_name = env_key_to_option_name(rest);
            values.insert(option_name, val);
        }
    }

    ConfigValues { values }
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

/// Looks up a value in a map using multiple key aliases for an option.
/// Checks camelCase (`opt.name`), kebab-case, and snake_case.
fn lookup_with_aliases(map: &HashMap<String, String>, opt: &Opt) -> Option<String> {
    let camel = &opt.name;
    if let Some(v) = map.get(camel) {
        return Some(v.clone());
    }
    let kebab = crate::parser::to_kebab(camel);
    if kebab != *camel && let Some(v) = map.get(&kebab) {
        return Some(v.clone());
    }
    let snake = kebab.replace('-', "_");
    if snake != *camel && snake != kebab && let Some(v) = map.get(&snake) {
        return Some(v.clone());
    }
    None
}

/// Builds effective options with config waterfall defaults applied.
///
/// For each option, resolves the effective default by layering:
/// 1. Code default (`opt.default`) — lowest priority
/// 2. TOML config value — overrides code default
/// 3. Environment variable — overrides TOML
///
/// CLI arguments override these during normal parsing (highest priority).
pub fn apply_waterfall(
    opts: &[Opt],
    toml_config: &ConfigValues,
    env_config: &ConfigValues,
) -> Vec<Opt> {
    opts.iter()
        .map(|opt| {
            let mut opt = opt.clone();

            // Layer 2: TOML overrides code default (supports camelCase, kebab-case, snake_case keys)
            if let Some(toml_val) = lookup_with_aliases(&toml_config.values, &opt) {
                opt.default = Some(toml_val);
            }

            // Layer 3: Env overrides TOML (and code default)
            // Check explicit env var name first, then prefix-loaded values
            let env_val = if let Some(ref explicit) = opt.env {
                std::env::var(explicit).ok()
            } else {
                lookup_with_aliases(&env_config.values, &opt)
            };

            if let Some(val) = env_val {
                opt.default = Some(val);
            }

            opt
        })
        .collect()
}

/// Merges config sources with waterfall precedence.
/// Returns the final resolved values for options not set via CLI.
pub fn merge(
    toml_config: &ConfigValues,
    env_config: &ConfigValues,
    opts: &[Opt],
) -> HashMap<String, Value> {
    let mut result = HashMap::new();

    for opt in opts {
        // Start with code default
        let mut value: Option<String> = opt.default.clone();

        // TOML overrides code default (with alias support)
        if let Some(toml_val) = lookup_with_aliases(&toml_config.values, opt) {
            value = Some(toml_val);
        }

        // Env overrides TOML: explicit env var name first, then prefix-loaded values
        if let Some(ref explicit) = opt.env {
            if let Ok(v) = std::env::var(explicit) {
                value = Some(v);
            }
        } else if let Some(env_val) = lookup_with_aliases(&env_config.values, opt) {
            value = Some(env_val);
        }

        if let Some(val) = value {
            result.insert(opt.name.clone(), coerce(&val, opt.opt_type));
        }
    }

    result
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_toml_parses_file() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join("test.toml");
        std::fs::write(
            &toml_path,
            "timeout = 60\nverbose = true\nname = \"alice\"\n",
        )
        .unwrap();

        let config = load_toml_from_path(&toml_path);
        assert_eq!(config.values.get("timeout").unwrap(), "60");
        assert_eq!(config.values.get("verbose").unwrap(), "true");
        assert_eq!(config.values.get("name").unwrap(), "alice");
    }

    #[test]
    fn load_toml_missing_file() {
        let config = load_toml_from_path(Path::new("/nonexistent/path.toml"));
        assert!(config.values.is_empty());
    }

    #[test]
    fn load_toml_skips_nested() {
        let dir = tempfile::tempdir().unwrap();
        let toml_path = dir.path().join("test.toml");
        std::fs::write(&toml_path, "timeout = 60\n[nested]\nkey = \"val\"\n").unwrap();

        let config = load_toml_from_path(&toml_path);
        assert_eq!(config.values.len(), 1);
        assert_eq!(config.values.get("timeout").unwrap(), "60");
    }

    #[test]
    fn load_env_with_prefix() {
        // SAFETY: test-only env var manipulation
        unsafe {
            std::env::set_var("TEST_INCUR_TIMEOUT", "120");
            std::env::set_var("TEST_INCUR_VERBOSE", "true");
        }
        let config = load_env("TEST_INCUR");
        assert_eq!(config.values.get("timeout").unwrap(), "120");
        assert_eq!(config.values.get("verbose").unwrap(), "true");
        unsafe {
            std::env::remove_var("TEST_INCUR_TIMEOUT");
            std::env::remove_var("TEST_INCUR_VERBOSE");
        }
    }

    #[test]
    fn load_env_camel_case_conversion() {
        unsafe {
            std::env::set_var("TEST_INCUR2_SAVE_DEV", "true");
        }
        let config = load_env("TEST_INCUR2");
        assert_eq!(config.values.get("saveDev").unwrap(), "true");
        unsafe {
            std::env::remove_var("TEST_INCUR2_SAVE_DEV");
        }
    }

    #[test]
    fn merge_waterfall_precedence() {
        let mut toml_config = ConfigValues::default();
        toml_config.values.insert("timeout".into(), "30".into());
        toml_config.values.insert("name".into(), "from-toml".into());

        let mut env_config = ConfigValues::default();
        env_config.values.insert("timeout".into(), "60".into());

        let opts = vec![
            Opt::new("timeout").number().default_value("10"),
            Opt::new("name"),
        ];

        let merged = merge(&toml_config, &env_config, &opts);
        // env wins over toml for "timeout"
        assert_eq!(merged.get("timeout").unwrap().as_f64(), 60.0);
        // toml wins for "name" (no env override)
        assert_eq!(merged.get("name").unwrap().as_str(), "from-toml");
    }

    #[test]
    fn merge_code_default_lowest_priority() {
        let toml_config = ConfigValues::default();
        let env_config = ConfigValues::default();

        let opts = vec![Opt::new("timeout").number().default_value("10")];
        let merged = merge(&toml_config, &env_config, &opts);
        assert_eq!(merged.get("timeout").unwrap().as_f64(), 10.0);
    }

    #[test]
    fn env_key_to_option_name_simple() {
        assert_eq!(env_key_to_option_name("TIMEOUT"), "timeout");
        assert_eq!(env_key_to_option_name("SAVE_DEV"), "saveDev");
        assert_eq!(env_key_to_option_name("MAX_RETRY_COUNT"), "maxRetryCount");
    }

    #[test]
    fn apply_waterfall_layers() {
        let mut toml_config = ConfigValues::default();
        toml_config.values.insert("timeout".into(), "30".into());

        let mut env_config = ConfigValues::default();
        env_config.values.insert("timeout".into(), "60".into());

        let opts = vec![
            Opt::new("timeout").number().default_value("10"),
            Opt::new("name").default_value("default"),
        ];

        let effective = apply_waterfall(&opts, &toml_config, &env_config);
        // env wins for timeout
        assert_eq!(effective[0].default.as_deref(), Some("60"));
        // code default preserved for name (not in toml or env)
        assert_eq!(effective[1].default.as_deref(), Some("default"));
    }

    #[test]
    fn apply_waterfall_toml_overrides_code_default() {
        let mut toml_config = ConfigValues::default();
        toml_config.values.insert("name".into(), "from-toml".into());

        let env_config = ConfigValues::default();

        let opts = vec![Opt::new("name").default_value("code-default")];
        let effective = apply_waterfall(&opts, &toml_config, &env_config);
        assert_eq!(effective[0].default.as_deref(), Some("from-toml"));
    }

    #[test]
    fn apply_waterfall_explicit_env() {
        let toml_config = ConfigValues::default();
        let env_config = ConfigValues::default();

        unsafe {
            std::env::set_var("MY_CUSTOM_VAR", "from-env");
        }
        let opts = vec![Opt::new("timeout").env("MY_CUSTOM_VAR")];
        let effective = apply_waterfall(&opts, &toml_config, &env_config);
        assert_eq!(effective[0].default.as_deref(), Some("from-env"));
        unsafe {
            std::env::remove_var("MY_CUSTOM_VAR");
        }
    }

    #[test]
    fn toml_alias_kebab_case() {
        let mut toml_config = ConfigValues::default();
        toml_config.values.insert("save-dev".into(), "true".into());

        let env_config = ConfigValues::default();
        let opts = vec![Opt::new("saveDev").boolean()];

        let effective = apply_waterfall(&opts, &toml_config, &env_config);
        assert_eq!(effective[0].default.as_deref(), Some("true"));
    }

    #[test]
    fn toml_alias_snake_case() {
        let mut toml_config = ConfigValues::default();
        toml_config.values.insert("save_dev".into(), "true".into());

        let env_config = ConfigValues::default();
        let opts = vec![Opt::new("saveDev").boolean()];

        let effective = apply_waterfall(&opts, &toml_config, &env_config);
        assert_eq!(effective[0].default.as_deref(), Some("true"));
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
}
