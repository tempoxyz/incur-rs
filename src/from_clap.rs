//! Bridge from `clap::Command` trees to incur MCP tools.
//!
//! This module walks a clap command tree, extracts leaf subcommands, and
//! generates incur `ToolEntry` descriptors so the CLI can be exposed as an
//! MCP server with `--mcp`.
//!
//! Each leaf subcommand becomes one MCP tool whose handler spawns the current
//! executable with the appropriate subcommand + arguments, capturing
//! stdout/stderr. This avoids protocol corruption on the MCP stdio channel.

use std::env;
use std::sync::Arc;

use serde_json::Value;

use crate::cli::{CommandContext, CommandResult};
use crate::mcp::{self, ToolEntry};
use crate::parser::{Arg, Opt, OptType};

/// Walks a `clap::Command` tree and collects leaf subcommands as incur
/// `ToolEntry` values suitable for MCP serving.
///
/// Tool names are derived by joining the subcommand path with `_`
/// (e.g. `pr list` → `pr_list`).
///
/// Each tool accepts a single `argv` array parameter. The handler spawns
/// the current executable with the resolved subcommand path + argv,
/// capturing output.
pub fn tools_from_clap(cmd: &clap::Command) -> Vec<ToolEntry> {
    let mut tools = Vec::new();
    collect_leaves(cmd, &[], &mut tools);
    tools
}

/// Returns the CLI name and version extracted from a `clap::Command`.
pub fn cli_meta(cmd: &clap::Command) -> (String, String) {
    let name = cmd.get_name().to_string();
    let version = cmd
        .get_version()
        .unwrap_or("0.0.0")
        .to_string();
    (name, version)
}

/// Recursively walk the command tree. Leaf commands (those with no
/// subcommands, or whose subcommands are all hidden) produce tool entries.
fn collect_leaves(cmd: &clap::Command, path: &[&str], out: &mut Vec<ToolEntry>) {
    let subs: Vec<_> = cmd
        .get_subcommands()
        .filter(|s| !s.is_hide_set())
        .collect();

    if subs.is_empty() {
        // Leaf command — register as a tool.
        out.push(make_tool_entry(cmd, path));
    } else {
        for sub in subs {
            let mut child_path = path.to_vec();
            child_path.push(sub.get_name());
            collect_leaves(sub, &child_path, out);
        }
    }
}

/// Build an incur `ToolEntry` for a single leaf command.
fn make_tool_entry(cmd: &clap::Command, path: &[&str]) -> ToolEntry {
    let tool_name = if path.is_empty() {
        cmd.get_name().to_string()
    } else {
        path.join("_")
    };
    let description = cmd
        .get_about()
        .map(|s| s.to_string())
        .or_else(|| cmd.get_long_about().map(|s| s.to_string()));

    // Extract args and options from clap metadata for richer schemas.
    let (args, options) = extract_schema(cmd);

    let input_schema = mcp::build_input_schema(&args, &options);
    let tool = mcp::make_tool(tool_name, description, input_schema);

    // The subcommand path to prepend when spawning.
    let subcmd_path: Vec<String> = path.iter().map(|s| s.to_string()).collect();

    let args_clone = args.clone();
    let opts_clone = options.clone();

    let run: Box<dyn Fn(CommandContext) -> CommandResult + Send + Sync> =
        Box::new(move |ctx: CommandContext| {
            // Reconstruct argv from the tool call parameters.
            let mut argv: Vec<String> = subcmd_path.clone();

            // Add positional args in order.
            for arg in &args_clone {
                if let Some(val) = ctx.parsed().args.get(&arg.name) {
                    if !val.is_empty() {
                        argv.push(val.clone());
                    }
                }
            }

            // Add named options.
            for opt in &opts_clone {
                if let Some(val) = ctx.parsed().options.get(&opt.name) {
                    let kebab = crate::parser::to_kebab(&opt.name);
                    match opt.opt_type {
                        OptType::Bool => {
                            if val.as_bool() {
                                argv.push(format!("--{kebab}"));
                            }
                        }
                        OptType::Array => {
                            if let crate::parser::Value::Array(items) = val {
                                for item in items {
                                    argv.push(format!("--{kebab}"));
                                    argv.push(item.clone());
                                }
                            }
                        }
                        _ => {
                            let s = val.as_str();
                            if !s.is_empty() {
                                argv.push(format!("--{kebab}"));
                                argv.push(s.to_string());
                            }
                        }
                    }
                }
            }

            // Spawn the current exe.
            let exe = env::current_exe().unwrap_or_else(|_| "forge".into());
            let output = std::process::Command::new(&exe)
                .args(&argv)
                .output();

            match output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                    let code = out.status.code().unwrap_or(-1);

                    if out.status.success() {
                        CommandContext::ok(serde_json::json!({
                            "exitCode": code,
                            "stdout": stdout.trim_end(),
                            "stderr": stderr.trim_end(),
                        }))
                    } else {
                        CommandResult::Err {
                            code: format!("exit_{code}"),
                            message: if stderr.is_empty() {
                                stdout
                            } else {
                                stderr
                            },
                            retryable: false,
                            cta: None,
                        }
                    }
                }
                Err(e) => CommandResult::Err {
                    code: "spawn_error".into(),
                    message: e.to_string(),
                    retryable: false,
                    cta: None,
                },
            }
        });

    ToolEntry {
        tool,
        args,
        options,
        run: Arc::new(run),
    }
}

/// Extract positional arguments and named options from clap metadata.
fn extract_schema(cmd: &clap::Command) -> (Vec<Arg>, Vec<Opt>) {
    let mut args = Vec::new();
    let mut options = Vec::new();

    for clap_arg in cmd.get_arguments() {
        let id = clap_arg.get_id().as_str();
        // Skip clap built-ins.
        if matches!(id, "help" | "version" | "verbose" | "verbosity" | "quiet") {
            continue;
        }

        let desc = clap_arg
            .get_help()
            .map(|s| s.to_string())
            .unwrap_or_default();

        if clap_arg.is_positional() {
            let mut arg = Arg::new(id).description(desc);
            if clap_arg.is_required_set() {
                arg = arg.required(true);
            }
            if let Some(vals) = clap_arg.get_default_values().first() {
                arg = arg.default_value(vals.to_string_lossy().to_string());
            }
            args.push(arg);
        } else {
            let mut opt = Opt::new(id).description(desc);

            // Determine type.
            let action = clap_arg.get_action();
            match action {
                clap::ArgAction::SetTrue | clap::ArgAction::SetFalse | clap::ArgAction::Count => {
                    opt = opt.boolean();
                }
                clap::ArgAction::Append => {
                    opt = opt.array();
                }
                _ => {
                    // String by default.
                }
            }

            // Short flag.
            if let Some(short) = clap_arg.get_short() {
                opt = opt.short(short);
            }

            if clap_arg.is_required_set() {
                opt = opt.required(true);
            }

            if let Some(vals) = clap_arg.get_default_values().first() {
                opt = opt.default_value(vals.to_string_lossy().to_string());
            }

            // Possible values → enum_values.
            let possible: Vec<String> = clap_arg
                .get_possible_values()
                .iter()
                .filter(|pv| !pv.is_hide_set())
                .map(|pv| pv.get_name().to_string())
                .collect();
            if !possible.is_empty() {
                opt = opt.enum_values(possible);
            }

            options.push(opt);
        }
    }

    (args, options)
}

/// Check for `--mcp` or `--llms` in raw argv and handle them if present.
///
/// Call this **before** `clap::Parser::parse()`. If `--mcp` or `--llms` is
/// found, this function handles the request and exits. Otherwise it returns
/// `Ok(())` and you proceed with normal clap parsing.
///
/// # Example
///
/// ```rust,no_run
/// use clap::{CommandFactory, Parser};
///
/// #[derive(Parser)]
/// #[command(name = "forge", version = "1.0.0")]
/// struct Forge {
///     #[command(subcommand)]
///     cmd: ForgeCmd,
/// }
///
/// #[derive(clap::Subcommand)]
/// enum ForgeCmd {
///     Build,
/// }
///
/// fn main() {
///     // Intercept --mcp/--llms before clap parsing
///     incur::from_clap::intercept::<Forge>();
///     // Normal clap flow continues...
///     let args = Forge::parse();
/// }
/// ```
pub fn intercept<C: clap::CommandFactory>() {
    let argv: Vec<String> = env::args().collect();
    if argv.iter().any(|a| a == "--mcp") {
        let cmd = C::command();
        let tools = tools_from_clap(&cmd);
        let (name, version) = cli_meta(&cmd);

        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async {
            if let Err(e) = crate::mcp::serve(name, version, tools).await {
                eprintln!("MCP server error: {e}");
                std::process::exit(1);
            }
        });
        std::process::exit(0);
    }

    if argv.iter().any(|a| a == "--llms") {
        let cmd = C::command();
        print!("{}", llms_manifest(&cmd));
        std::process::exit(0);
    }
}

/// Format an `--llms` manifest for a clap command tree.
///
/// Produces a Markdown document listing all available subcommands with
/// their descriptions and arguments, suitable for LLM consumption.
pub fn llms_manifest(cmd: &clap::Command) -> String {
    let name = cmd.get_name();
    let desc = cmd
        .get_about()
        .map(|s| s.to_string())
        .unwrap_or_default();
    let version = cmd.get_version().unwrap_or("0.0.0");

    let mut lines = vec![
        format!("# {name}"),
        String::new(),
        desc,
        format!("Version: {version}"),
        String::new(),
        "## Commands".to_string(),
        String::new(),
    ];

    let mut entries = Vec::new();
    collect_manifest_entries(cmd, &[], &mut entries);

    for (path, about, usage) in &entries {
        lines.push(format!("### `{path}`"));
        if !about.is_empty() {
            lines.push(about.clone());
        }
        if !usage.is_empty() {
            lines.push(format!("```\n{usage}\n```"));
        }
        lines.push(String::new());
    }

    lines.join("\n")
}

fn collect_manifest_entries(
    cmd: &clap::Command,
    path: &[&str],
    out: &mut Vec<(String, String, String)>,
) {
    let subs: Vec<_> = cmd
        .get_subcommands()
        .filter(|s| !s.is_hide_set())
        .collect();

    if subs.is_empty() && !path.is_empty() {
        let full_path = path.join(" ");
        let about = cmd
            .get_about()
            .map(|s| s.to_string())
            .unwrap_or_default();

        // Build a concise usage string.
        let mut usage_parts = vec![full_path.clone()];
        for arg in cmd.get_arguments() {
            let id = arg.get_id().as_str();
            if matches!(id, "help" | "version") {
                continue;
            }
            if arg.is_positional() {
                if arg.is_required_set() {
                    usage_parts.push(format!("<{id}>"));
                } else {
                    usage_parts.push(format!("[{id}]"));
                }
            } else if let Some(short) = arg.get_short() {
                usage_parts.push(format!("-{short}/--{id}"));
            } else {
                usage_parts.push(format!("--{id}"));
            }
        }
        let usage = usage_parts.join(" ");

        out.push((full_path, about, usage));
    } else {
        for sub in subs {
            let mut child_path = path.to_vec();
            child_path.push(sub.get_name());
            collect_manifest_entries(sub, &child_path, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_clap_cmd() -> clap::Command {
        clap::Command::new("my-cli")
            .version("1.0.0")
            .about("A test CLI")
            .subcommand(
                clap::Command::new("build")
                    .about("Build the project")
                    .arg(
                        clap::Arg::new("target")
                            .help("Build target")
                            .required(true),
                    )
                    .arg(
                        clap::Arg::new("release")
                            .long("release")
                            .short('r')
                            .help("Build in release mode")
                            .action(clap::ArgAction::SetTrue),
                    ),
            )
            .subcommand(
                clap::Command::new("test")
                    .about("Run tests")
                    .arg(
                        clap::Arg::new("filter")
                            .help("Test name filter"),
                    )
                    .arg(
                        clap::Arg::new("jobs")
                            .long("jobs")
                            .short('j')
                            .help("Number of parallel jobs"),
                    ),
            )
            .subcommand(
                clap::Command::new("pr")
                    .about("PR commands")
                    .subcommand(
                        clap::Command::new("list")
                            .about("List PRs")
                            .arg(
                                clap::Arg::new("state")
                                    .long("state")
                                    .help("Filter by state")
                                    .value_parser(["open", "closed", "all"])
                                    .default_value("open"),
                            ),
                    )
                    .subcommand(
                        clap::Command::new("create")
                            .about("Create a PR")
                            .arg(
                                clap::Arg::new("title")
                                    .help("PR title")
                                    .required(true),
                            ),
                    ),
            )
    }

    #[test]
    fn extracts_leaf_tools() {
        let cmd = sample_clap_cmd();
        let tools = tools_from_clap(&cmd);
        let names: Vec<&str> = tools.iter().map(|t| t.tool.name.as_ref()).collect();

        assert_eq!(names.len(), 4);
        assert!(names.contains(&"build"));
        assert!(names.contains(&"test"));
        assert!(names.contains(&"pr_list"));
        assert!(names.contains(&"pr_create"));
    }

    #[test]
    fn tool_descriptions() {
        let cmd = sample_clap_cmd();
        let tools = tools_from_clap(&cmd);

        let build = tools.iter().find(|t| &*t.tool.name == "build").unwrap();
        assert_eq!(build.tool.description.as_deref(), Some("Build the project"));

        let pr_list = tools.iter().find(|t| &*t.tool.name == "pr_list").unwrap();
        assert_eq!(pr_list.tool.description.as_deref(), Some("List PRs"));
    }

    #[test]
    fn extracts_args_and_options() {
        let cmd = sample_clap_cmd();
        let tools = tools_from_clap(&cmd);

        let build = tools.iter().find(|t| &*t.tool.name == "build").unwrap();
        assert_eq!(build.args.len(), 1);
        assert_eq!(build.args[0].name, "target");
        assert!(build.args[0].required);

        assert_eq!(build.options.len(), 1);
        assert_eq!(build.options[0].name, "release");
        assert_eq!(build.options[0].opt_type, OptType::Bool);
        assert_eq!(build.options[0].short, Some('r'));
    }

    #[test]
    fn extracts_enum_values() {
        let cmd = sample_clap_cmd();
        let tools = tools_from_clap(&cmd);

        let pr_list = tools.iter().find(|t| &*t.tool.name == "pr_list").unwrap();
        let state_opt = pr_list.options.iter().find(|o| o.name == "state").unwrap();
        assert_eq!(state_opt.enum_values, vec!["open", "closed", "all"]);
        assert_eq!(state_opt.default.as_deref(), Some("open"));
    }

    #[test]
    fn cli_meta_extracts_name_version() {
        let cmd = sample_clap_cmd();
        let (name, version) = cli_meta(&cmd);
        assert_eq!(name, "my-cli");
        assert_eq!(version, "1.0.0");
    }

    #[test]
    fn input_schema_has_properties() {
        let cmd = sample_clap_cmd();
        let tools = tools_from_clap(&cmd);

        let build = tools.iter().find(|t| &*t.tool.name == "build").unwrap();
        let schema = Value::Object((*build.tool.input_schema).clone());
        assert_eq!(schema["type"], "object");

        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("target"));
        assert!(props.contains_key("release"));
        assert_eq!(props["target"]["type"], "string");
        assert_eq!(props["release"]["type"], "boolean");
    }

    #[test]
    fn llms_manifest_format() {
        let cmd = sample_clap_cmd();
        let manifest = llms_manifest(&cmd);

        assert!(manifest.contains("# my-cli"));
        assert!(manifest.contains("Version: 1.0.0"));
        assert!(manifest.contains("### `build`"));
        assert!(manifest.contains("Build the project"));
        assert!(manifest.contains("### `pr list`"));
        assert!(manifest.contains("### `pr create`"));
    }
}
