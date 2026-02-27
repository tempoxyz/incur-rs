use crate::parser::{Arg, Opt, to_kebab};

/// Formats help text for a router CLI (command list).
pub fn format_root(name: &str, opts: &FormatRootOptions) -> String {
    let mut lines = Vec::new();

    // Header
    if let Some(desc) = &opts.description {
        lines.push(format!("{name} \u{2014} {desc}"));
    } else {
        lines.push(name.to_string());
    }
    if let Some(ver) = &opts.version {
        lines.push(format!("v{ver}"));
    }
    lines.push(String::new());

    // Synopsis
    lines.push(format!("Usage: {name} <command>"));

    // Commands
    if !opts.commands.is_empty() {
        lines.push(String::new());
        lines.push("Commands:".into());
        let max_len = opts
            .commands
            .iter()
            .map(|c| c.name.len())
            .max()
            .unwrap_or(0);
        for cmd in &opts.commands {
            if let Some(desc) = &cmd.description {
                let padding = " ".repeat(max_len - cmd.name.len());
                lines.push(format!("  {}{padding}  {desc}", cmd.name));
            } else {
                lines.push(format!("  {}", cmd.name));
            }
        }
    }

    lines.extend(global_options_lines(opts.root));
    lines.join("\n")
}

/// Options for formatting root help.
pub struct FormatRootOptions {
    pub description: Option<String>,
    pub version: Option<String>,
    pub commands: Vec<CommandEntry>,
    pub root: bool,
}

/// A command entry for help output.
pub struct CommandEntry {
    pub name: String,
    pub description: Option<String>,
}

/// Formats help text for a leaf command.
pub fn format_command(name: &str, opts: &FormatCommandOptions) -> String {
    let mut lines = Vec::new();

    // Header
    if let Some(desc) = &opts.description {
        lines.push(format!("{name} \u{2014} {desc}"));
    } else {
        lines.push(name.to_string());
    }
    if let Some(ver) = &opts.version {
        lines.push(format!("v{ver}"));
    }
    lines.push(String::new());

    // Synopsis
    let synopsis = build_synopsis(name, &opts.args);
    let opt_suffix = if opts.options.is_empty() {
        ""
    } else {
        " [options]"
    };
    let cmd_suffix = if opts.subcommands.is_empty() {
        ""
    } else {
        " | <command>"
    };
    lines.push(format!("Usage: {synopsis}{opt_suffix}{cmd_suffix}"));

    // Arguments
    if !opts.args.is_empty() {
        lines.push(String::new());
        lines.push("Arguments:".into());
        let max_len = opts.args.iter().map(|a| a.name.len()).max().unwrap_or(0);
        for arg in &opts.args {
            let padding = " ".repeat(max_len - arg.name.len());
            lines.push(format!("  {}{padding}  {}", arg.name, arg.description));
        }
    }

    // Options
    if !opts.options.is_empty() {
        lines.push(String::new());
        lines.push("Options:".into());
        let entries: Vec<(String, String)> = opts
            .options
            .iter()
            .map(|o| {
                let kebab = to_kebab(&o.name);
                let type_name = o.opt_type.to_string();
                let flag = if let Some(c) = o.short {
                    format!("--{kebab}, -{c} <{type_name}>")
                } else {
                    format!("--{kebab} <{type_name}>")
                };
                let desc = if let Some(default) = &o.default {
                    format!("{} (default: {default})", o.description)
                } else {
                    o.description.clone()
                };
                (flag, desc)
            })
            .collect();
        let max_len = entries.iter().map(|(f, _)| f.len()).max().unwrap_or(0);
        for (flag, desc) in &entries {
            let padding = " ".repeat(max_len - flag.len());
            lines.push(format!("  {flag}{padding}  {desc}"));
        }
    }

    // Examples
    if !opts.examples.is_empty() {
        lines.push(String::new());
        lines.push("Examples:".into());
        let max_len = opts
            .examples
            .iter()
            .map(|e| {
                if e.command.is_empty() {
                    format!("$ {name}").len()
                } else {
                    format!("$ {name} {}", e.command).len()
                }
            })
            .max()
            .unwrap_or(0);
        for ex in &opts.examples {
            let cmd = if ex.command.is_empty() {
                format!("$ {name}")
            } else {
                format!("$ {name} {}", ex.command)
            };
            if let Some(desc) = &ex.description {
                let padding = " ".repeat(max_len - cmd.len());
                lines.push(format!("  {cmd}{padding}  {desc}"));
            } else {
                lines.push(format!("  {cmd}"));
            }
        }
    }

    // Hint
    if let Some(hint) = &opts.hint {
        lines.push(String::new());
        lines.push(hint.clone());
    }

    // Subcommands
    if !opts.subcommands.is_empty() {
        lines.push(String::new());
        lines.push("Commands:".into());
        let max_len = opts
            .subcommands
            .iter()
            .map(|c| c.name.len())
            .max()
            .unwrap_or(0);
        for cmd in &opts.subcommands {
            if let Some(desc) = &cmd.description {
                let padding = " ".repeat(max_len - cmd.name.len());
                lines.push(format!("  {}{padding}  {desc}", cmd.name));
            } else {
                lines.push(format!("  {}", cmd.name));
            }
        }
    }

    lines.extend(global_options_lines(opts.root));
    lines.join("\n")
}

/// Options for formatting command help.
pub struct FormatCommandOptions {
    pub description: Option<String>,
    pub version: Option<String>,
    pub args: Vec<Arg>,
    pub options: Vec<Opt>,
    pub examples: Vec<ExampleEntry>,
    pub hint: Option<String>,
    pub subcommands: Vec<CommandEntry>,
    pub root: bool,
}

/// An example entry for help output.
pub struct ExampleEntry {
    pub command: String,
    pub description: Option<String>,
}

fn build_synopsis(name: &str, args: &[Arg]) -> String {
    if args.is_empty() {
        return name.to_string();
    }
    let mut parts = vec![name.to_string()];
    for arg in args {
        if arg.required {
            parts.push(format!("<{}>", arg.name));
        } else {
            parts.push(format!("[{}]", arg.name));
        }
    }
    parts.join(" ")
}

fn global_options_lines(root: bool) -> Vec<String> {
    let mut lines = Vec::new();

    if root {
        lines.push(String::new());
        lines.push("Built-in Commands:".into());
        lines.push("  mcp add      Register as an MCP server".into());
        lines.push("  skills add   Sync skill files to your agent".into());
    }

    let mut flags = vec![
        ("--format <toon|json|yaml|md|jsonl>", "Output format"),
        ("--help", "Show help"),
        ("--llms", "Print LLM-readable manifest"),
    ];
    if root {
        flags.push(("--mcp", "Start as MCP stdio server"));
    }
    flags.push(("--verbose", "Show full output envelope"));
    if root {
        flags.push(("--version", "Show version"));
    }

    let max_len = flags.iter().map(|(f, _)| f.len()).max().unwrap_or(0);
    lines.push(String::new());
    lines.push("Global Options:".into());
    for (flag, desc) in &flags {
        let padding = " ".repeat(max_len - flag.len());
        lines.push(format!("  {flag}{padding}  {desc}"));
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_help() {
        let help = format_root(
            "my-cli",
            &FormatRootOptions {
                description: Some("My CLI".into()),
                version: Some("1.0.0".into()),
                commands: vec![
                    CommandEntry {
                        name: "status".into(),
                        description: Some("Show status".into()),
                    },
                    CommandEntry {
                        name: "install".into(),
                        description: Some("Install a package".into()),
                    },
                ],
                root: true,
            },
        );
        assert!(help.contains("my-cli \u{2014} My CLI"));
        assert!(help.contains("v1.0.0"));
        assert!(help.contains("status"));
        assert!(help.contains("install"));
    }

    #[test]
    fn command_help() {
        let help = format_command(
            "greet",
            &FormatCommandOptions {
                description: Some("A greeting CLI".into()),
                version: None,
                args: vec![Arg::new("name").description("Name to greet").required(true)],
                options: vec![],
                examples: vec![],
                hint: None,
                subcommands: vec![],
                root: false,
            },
        );
        assert!(help.contains("greet \u{2014} A greeting CLI"));
        assert!(help.contains("Usage: greet <name>"));
        assert!(help.contains("name  Name to greet"));
    }
}
