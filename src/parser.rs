use std::collections::HashMap;

use crate::error::{FieldError, IncurError};

/// Defines a positional argument.
#[derive(Debug, Clone)]
pub struct Arg {
    pub name: String,
    pub description: String,
    pub required: bool,
    pub default: Option<String>,
}

impl Arg {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            required: false,
            default: None,
        }
    }

    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    pub fn default_value(mut self, val: impl Into<String>) -> Self {
        self.default = Some(val.into());
        self
    }
}

/// The type of an option's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptType {
    String,
    Number,
    Bool,
    Array,
}

impl std::fmt::Display for OptType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String => write!(f, "string"),
            Self::Number => write!(f, "number"),
            Self::Bool => write!(f, "boolean"),
            Self::Array => write!(f, "array"),
        }
    }
}

/// Defines a named option/flag.
#[derive(Debug, Clone)]
pub struct Opt {
    pub name: String,
    pub description: String,
    pub short: Option<char>,
    pub opt_type: OptType,
    pub required: bool,
    pub default: Option<String>,
    pub enum_values: Vec<String>,
}

impl Opt {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            short: None,
            opt_type: OptType::String,
            required: false,
            default: None,
            enum_values: Vec::new(),
        }
    }

    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn short(mut self, c: char) -> Self {
        self.short = Some(c);
        self
    }

    pub fn opt_type(mut self, t: OptType) -> Self {
        self.opt_type = t;
        self
    }

    pub fn boolean(mut self) -> Self {
        self.opt_type = OptType::Bool;
        self
    }

    pub fn number(mut self) -> Self {
        self.opt_type = OptType::Number;
        self
    }

    pub fn array(mut self) -> Self {
        self.opt_type = OptType::Array;
        self
    }

    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    pub fn default_value(mut self, val: impl Into<String>) -> Self {
        self.default = Some(val.into());
        self
    }

    pub fn enum_values(mut self, values: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.enum_values = values.into_iter().map(Into::into).collect();
        self
    }
}

/// Parsed arguments and options from argv.
#[derive(Debug, Clone, Default)]
pub struct Parsed {
    pub args: HashMap<String, String>,
    pub options: HashMap<String, Value>,
}

/// A parsed option value.
#[derive(Debug, Clone)]
pub enum Value {
    String(String),
    Bool(bool),
    Number(f64),
    Array(Vec<String>),
}

impl Value {
    pub fn as_str(&self) -> &str {
        match self {
            Self::String(s) => s,
            Self::Bool(b) => {
                if *b {
                    "true"
                } else {
                    "false"
                }
            }
            Self::Number(_) => "",
            Self::Array(_) => "",
        }
    }

    pub fn as_bool(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::String(s) => s == "true" || s == "1",
            Self::Number(n) => *n != 0.0,
            Self::Array(a) => !a.is_empty(),
        }
    }

    pub fn as_f64(&self) -> f64 {
        match self {
            Self::Number(n) => *n,
            Self::String(s) => s.parse().unwrap_or(0.0),
            Self::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Self::Array(_) => 0.0,
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String(s) => write!(f, "{s}"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Number(n) => {
                if *n == (*n as i64) as f64 {
                    write!(f, "{}", *n as i64)
                } else {
                    write!(f, "{n}")
                }
            }
            Self::Array(a) => write!(f, "{}", a.join(",")),
        }
    }
}

/// Converts camelCase to kebab-case.
pub fn to_kebab(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('-');
            result.push(c.to_lowercase().next().unwrap());
        } else {
            result.push(c);
        }
    }
    result
}

/// Parses argv tokens against arg and option definitions.
pub fn parse(
    argv: &[String],
    args_schema: &[Arg],
    opts_schema: &[Opt],
) -> Result<Parsed, IncurError> {
    // Build lookup maps
    let mut short_to_name: HashMap<char, String> = HashMap::new();
    let mut opt_by_name: HashMap<String, &Opt> = HashMap::new();
    let mut kebab_to_name: HashMap<String, String> = HashMap::new();

    for opt in opts_schema {
        opt_by_name.insert(opt.name.clone(), opt);
        if let Some(c) = opt.short {
            short_to_name.insert(c, opt.name.clone());
        }
        let kebab = to_kebab(&opt.name);
        if kebab != opt.name {
            kebab_to_name.insert(kebab, opt.name.clone());
        }
    }

    let resolve_name = |raw: &str| -> String {
        kebab_to_name
            .get(raw)
            .cloned()
            .unwrap_or_else(|| raw.to_string())
    };

    let mut positionals: Vec<String> = Vec::new();
    let mut raw_options: HashMap<String, Value> = HashMap::new();

    let mut i = 0;
    while i < argv.len() {
        let token = &argv[i];

        if token.starts_with("--no-") && token.len() > 5 {
            let raw = &token[5..];
            let name = resolve_name(raw);
            if !opt_by_name.contains_key(&name) {
                return Err(IncurError::Parse {
                    message: format!("Unknown flag: {token}"),
                });
            }
            raw_options.insert(name, Value::Bool(false));
            i += 1;
        } else if token.starts_with("--") {
            let rest = &token[2..];
            if let Some(eq_idx) = rest.find('=') {
                let raw = &rest[..eq_idx];
                let val = &rest[eq_idx + 1..];
                let name = resolve_name(raw);
                if !opt_by_name.contains_key(&name) {
                    return Err(IncurError::Parse {
                        message: format!("Unknown flag: --{raw}"),
                    });
                }
                set_option(&mut raw_options, &name, val, opt_by_name.get(&name));
                i += 1;
            } else {
                let name = resolve_name(rest);
                if !opt_by_name.contains_key(&name) {
                    return Err(IncurError::Parse {
                        message: format!("Unknown flag: {token}"),
                    });
                }
                if opt_by_name
                    .get(&name)
                    .is_some_and(|o| o.opt_type == OptType::Bool)
                {
                    raw_options.insert(name, Value::Bool(true));
                    i += 1;
                } else {
                    let value = argv.get(i + 1).ok_or_else(|| IncurError::Parse {
                        message: format!("Missing value for flag: {token}"),
                    })?;
                    set_option(&mut raw_options, &name, value, opt_by_name.get(&name));
                    i += 2;
                }
            }
        } else if token.starts_with('-') && token.len() == 2 {
            let c = token.chars().nth(1).unwrap();
            let name = short_to_name.get(&c).ok_or_else(|| IncurError::Parse {
                message: format!("Unknown flag: {token}"),
            })?;
            if opt_by_name
                .get(name.as_str())
                .is_some_and(|o| o.opt_type == OptType::Bool)
            {
                raw_options.insert(name.clone(), Value::Bool(true));
                i += 1;
            } else {
                let value = argv.get(i + 1).ok_or_else(|| IncurError::Parse {
                    message: format!("Missing value for flag: {token}"),
                })?;
                set_option(
                    &mut raw_options,
                    name,
                    value,
                    opt_by_name.get(name.as_str()),
                );
                i += 2;
            }
        } else {
            positionals.push(token.clone());
            i += 1;
        }
    }

    // Assign positionals to args schema keys
    let mut parsed_args: HashMap<String, String> = HashMap::new();
    for (j, arg) in args_schema.iter().enumerate() {
        if let Some(val) = positionals.get(j) {
            parsed_args.insert(arg.name.clone(), val.clone());
        } else if let Some(default) = &arg.default {
            parsed_args.insert(arg.name.clone(), default.clone());
        } else if arg.required {
            return Err(IncurError::Validation {
                message: format!("missing required argument <{}>", arg.name),
                field_errors: vec![FieldError {
                    path: arg.name.clone(),
                    expected: "string".into(),
                    received: String::new(),
                    message: format!("Required argument '{}' is missing", arg.name),
                }],
            });
        }
    }

    // Apply defaults for missing options
    for opt in opts_schema {
        if !raw_options.contains_key(&opt.name) {
            if let Some(default) = &opt.default {
                let val = coerce(default, opt.opt_type);
                raw_options.insert(opt.name.clone(), val);
            }
        }
    }

    // Validate required options
    for opt in opts_schema {
        if opt.required && !raw_options.contains_key(&opt.name) {
            return Err(IncurError::Validation {
                message: format!("missing required option --{}", to_kebab(&opt.name)),
                field_errors: vec![FieldError {
                    path: opt.name.clone(),
                    expected: opt.opt_type.to_string(),
                    received: String::new(),
                    message: format!("Required option '{}' is missing", opt.name),
                }],
            });
        }
    }

    // Validate enum values
    for opt in opts_schema {
        if !opt.enum_values.is_empty() {
            if let Some(Value::String(val)) = raw_options.get(&opt.name) {
                if !opt.enum_values.contains(val) {
                    return Err(IncurError::Validation {
                        message: format!(
                            "invalid value '{}' for --{}. Expected one of: {}",
                            val,
                            to_kebab(&opt.name),
                            opt.enum_values.join(", ")
                        ),
                        field_errors: vec![FieldError {
                            path: opt.name.clone(),
                            expected: opt.enum_values.join(" | "),
                            received: val.clone(),
                            message: format!("Invalid value for '{}'", opt.name),
                        }],
                    });
                }
            }
        }
    }

    Ok(Parsed {
        args: parsed_args,
        options: raw_options,
    })
}

fn coerce(value: &str, opt_type: OptType) -> Value {
    match opt_type {
        OptType::Bool => Value::Bool(value == "true" || value == "1"),
        OptType::Number => Value::Number(value.parse().unwrap_or(0.0)),
        OptType::Array => Value::Array(vec![value.to_string()]),
        OptType::String => Value::String(value.to_string()),
    }
}

fn set_option(raw: &mut HashMap<String, Value>, name: &str, value: &str, opt: Option<&&Opt>) {
    let opt_type = opt.map(|o| o.opt_type).unwrap_or(OptType::String);
    if opt_type == OptType::Array {
        match raw.get_mut(name) {
            Some(Value::Array(arr)) => arr.push(value.to_string()),
            _ => {
                raw.insert(name.to_string(), Value::Array(vec![value.to_string()]));
            }
        }
    } else {
        raw.insert(name.to_string(), coerce(value, opt_type));
    }
}

/// Parses environment variables against a set of expected keys.
pub fn parse_env(
    keys: &[(&str, bool)], // (name, required)
) -> Result<HashMap<String, String>, IncurError> {
    let mut result = HashMap::new();
    for &(key, required) in keys {
        if let Ok(val) = std::env::var(key) {
            result.insert(key.to_string(), val);
        } else if required {
            return Err(IncurError::Validation {
                message: format!("missing required environment variable {key}"),
                field_errors: vec![FieldError {
                    path: key.to_string(),
                    expected: "string".into(),
                    received: String::new(),
                    message: format!("Environment variable '{key}' is not set"),
                }],
            });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_positional_args() {
        let argv: Vec<String> = vec!["hello".into(), "world".into()];
        let args = vec![
            Arg::new("greeting").required(true),
            Arg::new("target").required(true),
        ];
        let result = parse(&argv, &args, &[]).unwrap();
        assert_eq!(result.args.get("greeting").unwrap(), "hello");
        assert_eq!(result.args.get("target").unwrap(), "world");
    }

    #[test]
    fn parse_long_flag() {
        let argv: Vec<String> = vec!["--name".into(), "alice".into()];
        let opts = vec![Opt::new("name")];
        let result = parse(&argv, &[], &opts).unwrap();
        assert_eq!(result.options.get("name").unwrap().as_str(), "alice");
    }

    #[test]
    fn parse_short_flag() {
        let argv: Vec<String> = vec!["-n".into(), "bob".into()];
        let opts = vec![Opt::new("name").short('n')];
        let result = parse(&argv, &[], &opts).unwrap();
        assert_eq!(result.options.get("name").unwrap().as_str(), "bob");
    }

    #[test]
    fn parse_boolean_flag() {
        let argv: Vec<String> = vec!["--verbose".into()];
        let opts = vec![Opt::new("verbose").boolean()];
        let result = parse(&argv, &[], &opts).unwrap();
        assert!(result.options.get("verbose").unwrap().as_bool());
    }

    #[test]
    fn parse_negation() {
        let argv: Vec<String> = vec!["--no-verbose".into()];
        let opts = vec![Opt::new("verbose").boolean()];
        let result = parse(&argv, &[], &opts).unwrap();
        assert!(!result.options.get("verbose").unwrap().as_bool());
    }

    #[test]
    fn parse_equals_syntax() {
        let argv: Vec<String> = vec!["--name=alice".into()];
        let opts = vec![Opt::new("name")];
        let result = parse(&argv, &[], &opts).unwrap();
        assert_eq!(result.options.get("name").unwrap().as_str(), "alice");
    }

    #[test]
    fn parse_kebab_to_camel() {
        let argv: Vec<String> = vec!["--save-dev".into()];
        let opts = vec![Opt::new("saveDev").boolean()];
        let result = parse(&argv, &[], &opts).unwrap();
        assert!(result.options.get("saveDev").unwrap().as_bool());
    }

    #[test]
    fn parse_missing_required_arg() {
        let argv: Vec<String> = vec![];
        let args = vec![Arg::new("name").required(true)];
        let result = parse(&argv, &args, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn parse_unknown_flag() {
        let argv: Vec<String> = vec!["--unknown".into()];
        let result = parse(&argv, &[], &[]);
        assert!(result.is_err());
    }

    #[test]
    fn parse_array_option() {
        let argv: Vec<String> = vec!["--tag".into(), "a".into(), "--tag".into(), "b".into()];
        let opts = vec![Opt::new("tag").array()];
        let result = parse(&argv, &[], &opts).unwrap();
        match result.options.get("tag").unwrap() {
            Value::Array(arr) => assert_eq!(arr, &["a", "b"]),
            _ => panic!("expected array"),
        }
    }

    #[test]
    fn to_kebab_case() {
        assert_eq!(to_kebab("saveDev"), "save-dev");
        assert_eq!(to_kebab("name"), "name");
        assert_eq!(to_kebab("maxRetryCount"), "max-retry-count");
    }
}
