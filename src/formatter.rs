use serde_json::Value;

/// Supported output formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Toon,
    Json,
    Yaml,
    Md,
    Jsonl,
}

impl std::str::FromStr for Format {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "toon" => Ok(Self::Toon),
            "json" => Ok(Self::Json),
            "yaml" => Ok(Self::Yaml),
            "md" => Ok(Self::Md),
            "jsonl" => Ok(Self::Jsonl),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Toon => write!(f, "toon"),
            Self::Json => write!(f, "json"),
            Self::Yaml => write!(f, "yaml"),
            Self::Md => write!(f, "md"),
            Self::Jsonl => write!(f, "jsonl"),
        }
    }
}

/// Serializes a value to the specified format.
pub fn format(value: &Value, fmt: Format) -> String {
    if value.is_null() {
        return String::new();
    }
    match fmt {
        Format::Json => serde_json::to_string_pretty(value).unwrap_or_default(),
        Format::Yaml => serde_yaml::to_string(value)
            .unwrap_or_default()
            .trim_end()
            .to_string(),
        Format::Md => format_markdown(value, &[]),
        Format::Jsonl => serde_json::to_string(value).unwrap_or_default(),
        Format::Toon => format_toon(value, 0),
    }
}

/// TOON format — token-optimized object notation.
/// Like YAML but with no quoting, no braces, minimal syntax.
fn format_toon(value: &Value, indent: usize) -> String {
    let prefix = "  ".repeat(indent);
    match value {
        Value::Null => String::new(),
        Value::Bool(b) => format!("{b}"),
        Value::Number(n) => format!("{n}"),
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            if arr.is_empty() {
                return "(empty)".into();
            }
            // Check if array of flat objects → tabular TOON
            if arr.iter().all(|v| v.is_object() && is_flat(v)) {
                return format_toon_table(arr);
            }
            let mut lines = Vec::new();
            for item in arr {
                let formatted = format_toon(item, indent + 1);
                lines.push(format!("{prefix}- {}", formatted.trim_start()));
            }
            lines.join("\n")
        }
        Value::Object(obj) => {
            let mut lines = Vec::new();
            for (key, val) in obj {
                match val {
                    Value::Object(_) | Value::Array(_) if !is_simple(val) => {
                        lines.push(format!("{prefix}{key}:"));
                        lines.push(format_toon(val, indent + 1));
                    }
                    _ => {
                        let formatted = format_toon(val, 0);
                        lines.push(format!("{prefix}{key}: {formatted}"));
                    }
                }
            }
            lines.join("\n")
        }
    }
}

/// Formats an array of flat objects as a compact TOON table.
/// `items[3]{id,name,value}: 1,foo,bar / 2,baz,qux`
fn format_toon_table(arr: &[Value]) -> String {
    if arr.is_empty() {
        return "(empty)".into();
    }

    // Collect all keys from first object
    let keys: Vec<String> = if let Some(Value::Object(first)) = arr.first() {
        first.keys().cloned().collect()
    } else {
        return "(empty)".into();
    };

    let mut lines = Vec::new();
    for item in arr {
        if let Value::Object(obj) = item {
            let vals: Vec<String> = keys
                .iter()
                .map(|k| {
                    obj.get(k)
                        .map(|v| match v {
                            Value::String(s) => s.clone(),
                            Value::Null => String::new(),
                            other => other.to_string(),
                        })
                        .unwrap_or_default()
                })
                .collect();
            lines.push(format!("  {}", vals.join(",")));
        }
    }

    let header = format!("[{}]{{{}}}:", arr.len(), keys.join(","));
    format!("{header}\n{}", lines.join("\n"))
}

fn is_simple(value: &Value) -> bool {
    match value {
        Value::Object(obj) => obj.values().all(|v| !v.is_object() && !v.is_array()),
        Value::Array(arr) => arr.iter().all(|v| !v.is_object() && !v.is_array()),
        _ => true,
    }
}

fn is_flat(value: &Value) -> bool {
    match value {
        Value::Object(obj) => obj.values().all(|v| !v.is_object() && !v.is_array()),
        _ => false,
    }
}

/// Formats a value as Markdown.
fn format_markdown(value: &Value, path: &[&str]) -> String {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            let s = match value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            if path.is_empty() {
                s
            } else {
                format!("## {}\n\n{s}", path.join("."))
            }
        }
        Value::Array(arr) => {
            if is_array_of_objects(arr) {
                let table = columnar_table(arr);
                if path.is_empty() {
                    table
                } else {
                    format!("## {}\n\n{table}", path.join("."))
                }
            } else {
                let s = arr
                    .iter()
                    .map(|v| match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                if path.is_empty() {
                    s
                } else {
                    format!("## {}\n\n{s}", path.join("."))
                }
            }
        }
        Value::Object(obj) => {
            if path.is_empty() && obj.values().all(is_scalar) {
                return kv_table(obj);
            }
            let sections: Vec<String> = obj
                .iter()
                .map(|(key, val)| {
                    let mut child_path: Vec<&str> = path.to_vec();
                    child_path.push(key);
                    if is_scalar(val) {
                        let s = match val {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        format!("## {}\n\n{s}", child_path.join("."))
                    } else if let Value::Array(arr) = val {
                        if is_array_of_objects(arr) {
                            format!("## {}\n\n{}", child_path.join("."), columnar_table(arr))
                        } else {
                            format_markdown(val, &child_path)
                        }
                    } else if let Value::Object(nested) = val {
                        if nested.values().all(is_scalar) {
                            format!("## {}\n\n{}", child_path.join("."), kv_table(nested))
                        } else {
                            format_markdown(val, &child_path)
                        }
                    } else {
                        format_markdown(val, &child_path)
                    }
                })
                .collect();
            sections.join("\n\n")
        }
    }
}

fn is_scalar(value: &Value) -> bool {
    matches!(
        value,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
    )
}

fn is_array_of_objects(arr: &[Value]) -> bool {
    !arr.is_empty() && arr.iter().all(|v| v.is_object())
}

fn kv_table(obj: &serde_json::Map<String, Value>) -> String {
    let entries: Vec<(String, String)> = obj
        .iter()
        .map(|(k, v)| {
            let s = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            (k.clone(), s)
        })
        .collect();
    render_table(
        &["Key", "Value"],
        &entries
            .iter()
            .map(|(k, v)| vec![k.as_str(), v.as_str()])
            .collect::<Vec<_>>(),
    )
}

fn columnar_table(items: &[Value]) -> String {
    let mut keys: Vec<String> = Vec::new();
    for item in items {
        if let Value::Object(obj) = item {
            for key in obj.keys() {
                if !keys.contains(key) {
                    keys.push(key.clone());
                }
            }
        }
    }

    let rows: Vec<Vec<String>> = items
        .iter()
        .map(|item| {
            keys.iter()
                .map(|k| {
                    item.get(k)
                        .map(|v| match v {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .unwrap_or_default()
                })
                .collect()
        })
        .collect();

    let row_refs: Vec<Vec<&str>> = rows
        .iter()
        .map(|r| r.iter().map(String::as_str).collect())
        .collect();
    let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    render_table(&key_refs, &row_refs)
}

fn render_table(headers: &[&str], rows: &[Vec<&str>]) -> String {
    let widths: Vec<usize> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let max_row = rows
                .iter()
                .map(|r| r.get(i).map(|s| s.len()).unwrap_or(0))
                .max()
                .unwrap_or(0);
            h.len().max(max_row)
        })
        .collect();

    let header_row = format!(
        "| {} |",
        headers
            .iter()
            .enumerate()
            .map(|(i, h)| format!("{:width$}", h, width = widths[i]))
            .collect::<Vec<_>>()
            .join(" | ")
    );
    let sep = format!(
        "|{}|",
        widths
            .iter()
            .map(|w| "-".repeat(w + 2))
            .collect::<Vec<_>>()
            .join("|")
    );
    let body: Vec<String> = rows
        .iter()
        .map(|r| {
            format!(
                "| {} |",
                headers
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("{:width$}", r.get(i).unwrap_or(&""), width = widths[i]))
                    .collect::<Vec<_>>()
                    .join(" | ")
            )
        })
        .collect();

    format!("{header_row}\n{sep}\n{}", body.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn toon_flat_object() {
        let val = json!({"clean": true, "branch": "main"});
        let out = format(&val, Format::Toon);
        assert!(out.contains("clean: true"));
        assert!(out.contains("branch: main"));
    }

    #[test]
    fn json_format() {
        let val = json!({"ok": true});
        let out = format(&val, Format::Json);
        assert!(out.contains("\"ok\": true"));
    }

    #[test]
    fn toon_nested() {
        let val = json!({"context": {"task": "test"}, "count": 3});
        let out = format(&val, Format::Toon);
        assert!(out.contains("context:"));
        assert!(out.contains("task: test"));
        assert!(out.contains("count: 3"));
    }
}
