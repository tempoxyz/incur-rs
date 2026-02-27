use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;

use crate::cta::{self, CtaBlock};
use crate::error::IncurError;
use crate::formatter::{self, Format};
use crate::help::{self, CommandEntry, ExampleEntry, FormatCommandOptions, FormatRootOptions};
use crate::mcp;
use crate::parser::{self, Arg, Opt, Parsed};

/// The handler function type for a command.
pub type RunFn = Box<dyn Fn(CommandContext) -> CommandResult + Send + Sync>;

/// Result returned from a command handler.
pub enum CommandResult {
    /// Return data directly.
    Ok(Value),
    /// Return data with metadata (CTAs).
    OkWith { data: Value, cta: Option<CtaBlock> },
    /// Return an error.
    Err {
        code: String,
        message: String,
        retryable: bool,
        cta: Option<CtaBlock>,
    },
}

/// Context passed to command handlers.
pub struct CommandContext {
    parsed: Parsed,
    env: HashMap<String, String>,
}

impl CommandContext {
    /// Creates a new `CommandContext` from parsed data and environment.
    pub fn new(parsed: Parsed, env: HashMap<String, String>) -> Self {
        Self { parsed, env }
    }

    /// Gets a positional argument by name.
    pub fn arg<T: std::str::FromStr>(&self, name: &str) -> T
    where
        T::Err: std::fmt::Debug,
    {
        self.parsed
            .args
            .get(name)
            .expect(&format!("missing arg: {name}"))
            .parse()
            .expect(&format!("failed to parse arg: {name}"))
    }

    /// Gets a positional argument by name, returning None if missing.
    pub fn arg_opt<T: std::str::FromStr>(&self, name: &str) -> Option<T> {
        self.parsed.args.get(name).and_then(|v| v.parse().ok())
    }

    /// Gets a string argument by name.
    pub fn arg_str(&self, name: &str) -> &str {
        self.parsed.args.get(name).map(String::as_str).unwrap_or("")
    }

    /// Gets an option value by name.
    pub fn option(&self, name: &str) -> Option<&parser::Value> {
        self.parsed.options.get(name)
    }

    /// Gets a string option value.
    pub fn option_str(&self, name: &str) -> &str {
        self.parsed
            .options
            .get(name)
            .map(parser::Value::as_str)
            .unwrap_or("")
    }

    /// Gets a boolean option value.
    pub fn option_bool(&self, name: &str) -> bool {
        self.parsed
            .options
            .get(name)
            .map(parser::Value::as_bool)
            .unwrap_or(false)
    }

    /// Gets a numeric option value.
    pub fn option_f64(&self, name: &str) -> f64 {
        self.parsed
            .options
            .get(name)
            .map(parser::Value::as_f64)
            .unwrap_or(0.0)
    }

    /// Gets an environment variable by name.
    pub fn env(&self, name: &str) -> &str {
        self.env.get(name).map(String::as_str).unwrap_or("")
    }

    /// Returns a success result.
    pub fn ok(data: Value) -> CommandResult {
        CommandResult::Ok(data)
    }

    /// Returns a success result with CTAs.
    pub fn ok_with(data: Value, cta: CtaBlock) -> CommandResult {
        CommandResult::OkWith {
            data,
            cta: Some(cta),
        }
    }

    /// Returns an error result.
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> CommandResult {
        CommandResult::Err {
            code: code.into(),
            message: message.into(),
            retryable: false,
            cta: None,
        }
    }

    /// Access the raw parsed result.
    pub fn parsed(&self) -> &Parsed {
        &self.parsed
    }
}

/// A command definition within the CLI.
struct Command {
    description: Option<String>,
    args: Vec<Arg>,
    options: Vec<Opt>,
    #[allow(dead_code)]
    examples: Vec<ExampleEntry>,
    hint: Option<String>,
    format: Option<Format>,
    run: RunFn,
}

/// A command group (sub-CLI).
struct Group {
    description: Option<String>,
    commands: HashMap<String, Entry>,
    /// Insertion order for deterministic help output.
    order: Vec<String>,
}

enum Entry {
    Command(Command),
    Group(Group),
}

/// The CLI builder.
pub struct Cli {
    name: String,
    description: Option<String>,
    version: Option<String>,
    commands: HashMap<String, Entry>,
    order: Vec<String>,
    root_command: Option<Command>,
    default_format: Format,
}

impl Cli {
    /// Creates a new CLI with the given name.
    pub fn create(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            version: None,
            commands: HashMap::new(),
            order: Vec::new(),
            root_command: None,
            default_format: Format::Toon,
        }
    }

    /// Sets the CLI description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Sets the CLI version.
    pub fn version(mut self, ver: impl Into<String>) -> Self {
        self.version = Some(ver.into());
        self
    }

    /// Sets the default output format.
    pub fn format(mut self, fmt: Format) -> Self {
        self.default_format = fmt;
        self
    }

    /// Adds a positional argument (for root commands).
    pub fn arg(mut self, arg: Arg) -> Self {
        if let Some(cmd) = &mut self.root_command {
            cmd.args.push(arg);
        } else {
            // Create a placeholder root command; run will be set later
            self.root_command = Some(Command {
                description: None,
                args: vec![arg],
                options: Vec::new(),
                examples: Vec::new(),
                hint: None,
                format: None,
                run: Box::new(|_| CommandResult::Ok(Value::Null)),
            });
        }
        self
    }

    /// Adds a named option (for root commands).
    pub fn option(mut self, opt: Opt) -> Self {
        if let Some(cmd) = &mut self.root_command {
            cmd.options.push(opt);
        } else {
            self.root_command = Some(Command {
                description: None,
                args: Vec::new(),
                options: vec![opt],
                examples: Vec::new(),
                hint: None,
                format: None,
                run: Box::new(|_| CommandResult::Ok(Value::Null)),
            });
        }
        self
    }

    /// Sets the root command handler. Makes this a single-command CLI.
    pub fn run(
        mut self,
        f: impl Fn(CommandContext) -> CommandResult + Send + Sync + 'static,
    ) -> Self {
        if let Some(cmd) = &mut self.root_command {
            cmd.run = Box::new(f);
        } else {
            self.root_command = Some(Command {
                description: None,
                args: Vec::new(),
                options: Vec::new(),
                examples: Vec::new(),
                hint: None,
                format: None,
                run: Box::new(f),
            });
        }
        self
    }

    /// Registers a derived struct as a subcommand with its `IncurRun` handler.
    ///
    /// ```rust,ignore
    /// Cli::create("app")
    ///     .command_run::<Deploy>()
    ///     .command_run::<Status>()
    ///     .serve()
    ///     .await;
    /// ```
    pub fn command_run<T: crate::derive_support::IncurRun>(self) -> Self {
        let builder = T::command_builder().run(|ctx| T::from_context(&ctx).run());
        self.command(T::cli_name(), builder)
    }

    /// Registers a subcommand.
    pub fn command(mut self, name: impl Into<String>, builder: CommandBuilder) -> Self {
        let name = name.into();
        self.order.push(name.clone());
        self.commands.insert(
            name,
            Entry::Command(Command {
                description: builder.description,
                args: builder.args,
                options: builder.options,
                examples: builder.examples,
                hint: builder.hint,
                format: builder.format,
                run: builder.run.expect("command must have a run handler"),
            }),
        );
        self
    }

    /// Mounts a sub-CLI as a command group.
    pub fn group(mut self, sub: Cli) -> Self {
        let name = sub.name.clone();
        self.order.push(name.clone());

        let mut group = Group {
            description: sub.description,
            commands: HashMap::new(),
            order: sub.order,
        };

        for (k, v) in sub.commands {
            group.commands.insert(k, v);
        }

        // If the sub-CLI has a root command, add it as a direct entry
        if let Some(root_cmd) = sub.root_command {
            group.commands.insert(
                String::new(), // empty key = root of group
                Entry::Command(root_cmd),
            );
        }

        self.commands.insert(name, Entry::Group(group));
        self
    }

    /// Parses argv, runs the matched command, and writes output to stdout.
    pub async fn serve(self) {
        self.serve_with(std::env::args().skip(1).collect(), ServeOptions::default())
            .await;
    }

    /// Serves with explicit argv (useful for testing).
    pub async fn serve_argv(self, argv: Vec<String>) {
        self.serve_with(argv, ServeOptions::default()).await;
    }

    /// Serves with explicit argv and a custom stdout writer (for capturing output in tests).
    pub async fn serve_with_test(
        self,
        argv: Vec<String>,
        stdout: impl Fn(&str) + Send + Sync + 'static,
    ) {
        self.serve_with(
            argv,
            ServeOptions {
                stdout: Box::new(stdout),
                exit: Box::new(|_| {}),
            },
        )
        .await;
    }

    /// Internal serve implementation.
    async fn serve_with(self, argv: Vec<String>, opts: ServeOptions) {
        let stdout = opts.stdout;
        let exit = opts.exit;

        let flags = extract_builtin_flags(&argv);

        // --mcp: start as MCP stdio server
        if flags.mcp {
            let (name, version, tools) = self.collect_mcp_tools();
            if let Err(e) = mcp::serve(name, version, tools).await {
                eprintln!("MCP server error: {e}");
                (exit)(1);
            }
            return;
        }

        // --version
        if flags.version && !flags.help {
            if let Some(ver) = &self.version {
                writeln_fn(&stdout, ver);
                return;
            }
        }

        // --llms: output manifest (check before help/empty-args fallback)
        if flags.llms {
            let manifest = self.build_manifest();
            let fmt = if flags.format_explicit {
                flags.format
            } else {
                Format::Md
            };
            if fmt == Format::Md {
                writeln_fn(&stdout, &self.format_llms_md());
            } else {
                writeln_fn(&stdout, &formatter::format(&manifest, fmt));
            }
            return;
        }

        // --help with no command
        if flags.help && flags.rest.is_empty() {
            self.show_root_help(&stdout);
            return;
        }

        // No args → show root help (unless there's a root command)
        if flags.rest.is_empty() && self.root_command.is_none() {
            self.show_root_help(&stdout);
            return;
        }

        // Resolve command
        let resolved = self.resolve(&flags.rest);

        // --help after command tokens
        if flags.help {
            match &resolved {
                Resolved::Command { cmd, path, .. } => {
                    let full_name = if path.is_empty() {
                        self.name.clone()
                    } else {
                        format!("{} {path}", self.name)
                    };
                    let help_text = help::format_command(
                        &full_name,
                        &FormatCommandOptions {
                            description: cmd.description.clone(),
                            version: if path.is_empty() {
                                self.version.clone()
                            } else {
                                None
                            },
                            args: cmd.args.clone(),
                            options: cmd.options.clone(),
                            examples: Vec::new(), // TODO: carry examples
                            hint: cmd.hint.clone(),
                            subcommands: Vec::new(),
                            root: path.is_empty(),
                        },
                    );
                    writeln_fn(&stdout, &help_text);
                }
                Resolved::Group {
                    path,
                    description,
                    entries,
                    order,
                } => {
                    let full_name = format!("{} {path}", self.name);
                    let cmds = collect_help_commands(entries, order);
                    let help_text = help::format_root(
                        &full_name,
                        &FormatRootOptions {
                            description: description.clone(),
                            version: None,
                            commands: cmds,
                            root: false,
                        },
                    );
                    writeln_fn(&stdout, &help_text);
                }
                Resolved::NotFound { .. } => {
                    self.show_root_help(&stdout);
                }
                Resolved::Root { .. } => {
                    self.show_root_help(&stdout);
                }
            }
            return;
        }

        let human = !flags.format_explicit && !flags.verbose;
        let start = Instant::now();

        // Get the command to run
        let (cmd, path, rest) = match resolved {
            Resolved::Command { cmd, path, rest } => (cmd, path, rest),
            Resolved::Root { rest } => {
                if let Some(cmd) = &self.root_command {
                    (cmd, String::new(), rest)
                } else {
                    self.show_root_help(&stdout);
                    return;
                }
            }
            Resolved::Group {
                path,
                description,
                entries,
                order,
            } => {
                let full_name = format!("{} {path}", self.name);
                let cmds = collect_help_commands(&entries, &order);
                writeln_fn(
                    &stdout,
                    &help::format_root(
                        &full_name,
                        &FormatRootOptions {
                            description,
                            version: None,
                            commands: cmds,
                            root: false,
                        },
                    ),
                );
                return;
            }
            Resolved::NotFound { name: bad, path } => {
                // Fall back to root command if available
                if let Some(cmd) = &self.root_command {
                    (cmd, String::new(), flags.rest.clone())
                } else {
                    let help_cmd = if path.is_empty() {
                        format!("{} --help", self.name)
                    } else {
                        format!("{} {path} --help", self.name)
                    };
                    let msg = format!(
                        "'{bad}' is not a command. See '{help_cmd}' for available commands."
                    );
                    if human {
                        writeln_fn(&stdout, &format!("Error: {msg}"));
                    } else {
                        let err = serde_json::json!({
                            "ok": false,
                            "error": { "code": "COMMAND_NOT_FOUND", "message": msg },
                            "meta": { "command": bad, "duration": format_duration(start) }
                        });
                        writeln_fn(&stdout, &formatter::format(&err, flags.format));
                    }
                    exit(1);
                    return;
                }
            }
        };

        // Parse args and options
        let parsed = match parser::parse(&rest, &cmd.args, &cmd.options) {
            Ok(p) => p,
            Err(e) => {
                if human {
                    match &e {
                        IncurError::Validation { field_errors, .. } => {
                            let mut lines = Vec::new();
                            for fe in field_errors {
                                lines.push(format!(
                                    "Error: missing required argument <{}>",
                                    fe.path
                                ));
                            }
                            lines.push("See below for usage.".into());
                            lines.push(String::new());
                            let full_name = if path.is_empty() {
                                self.name.clone()
                            } else {
                                format!("{} {path}", self.name)
                            };
                            lines.push(help::format_command(
                                &full_name,
                                &FormatCommandOptions {
                                    description: cmd.description.clone(),
                                    version: None,
                                    args: cmd.args.clone(),
                                    options: cmd.options.clone(),
                                    examples: Vec::new(),
                                    hint: cmd.hint.clone(),
                                    subcommands: Vec::new(),
                                    root: false,
                                },
                            ));
                            writeln_fn(&stdout, &lines.join("\n"));
                        }
                        _ => {
                            writeln_fn(&stdout, &format!("Error: {e}"));
                        }
                    }
                } else {
                    let err = serde_json::json!({
                        "ok": false,
                        "error": { "code": "PARSE_ERROR", "message": e.to_string() },
                        "meta": { "command": path, "duration": format_duration(start) }
                    });
                    writeln_fn(&stdout, &formatter::format(&err, flags.format));
                }
                exit(1);
                return;
            }
        };

        let ctx = CommandContext {
            parsed,
            env: HashMap::new(),
        };

        let result = (cmd.run)(ctx);
        let fmt = if flags.format_explicit {
            flags.format
        } else {
            cmd.format.unwrap_or(self.default_format)
        };

        match result {
            CommandResult::Ok(data) => {
                let output = serde_json::json!({
                    "ok": true,
                    "data": data,
                    "meta": { "command": path, "duration": format_duration(start) }
                });
                if human {
                    if !data.is_null() {
                        writeln_fn(&stdout, &formatter::format(&data, fmt));
                    }
                } else if flags.verbose {
                    writeln_fn(&stdout, &formatter::format(&output, fmt));
                } else {
                    let formatted = formatter::format(&data, fmt);
                    if !formatted.is_empty() {
                        writeln_fn(&stdout, &formatted);
                    }
                }
            }
            CommandResult::OkWith { data, cta } => {
                if human {
                    if !data.is_null() {
                        writeln_fn(&stdout, &formatter::format(&data, fmt));
                    }
                    if let Some(cta) = &cta {
                        writeln_fn(&stdout, &cta::format_human(&self.name, cta));
                    }
                } else if flags.verbose {
                    let mut output = serde_json::json!({
                        "ok": true,
                        "data": data,
                        "meta": { "command": path, "duration": format_duration(start) }
                    });
                    if let Some(cta) = &cta {
                        output["meta"]["cta"] = serde_json::to_value(cta).unwrap_or_default();
                    }
                    writeln_fn(&stdout, &formatter::format(&output, fmt));
                } else {
                    let mut payload = if data.is_object() {
                        data.clone()
                    } else {
                        serde_json::json!({ "data": data })
                    };
                    if let Some(cta) = &cta {
                        payload["cta"] = serde_json::to_value(cta).unwrap_or_default();
                    }
                    writeln_fn(&stdout, &formatter::format(&payload, fmt));
                }
            }
            CommandResult::Err {
                code,
                message,
                retryable,
                cta,
            } => {
                if human {
                    writeln_fn(&stdout, &format!("Error [{code}]: {message}"));
                    if let Some(cta) = &cta {
                        writeln_fn(&stdout, &cta::format_human(&self.name, cta));
                    }
                } else {
                    let mut err = serde_json::json!({
                        "ok": false,
                        "error": { "code": code, "message": message, "retryable": retryable },
                        "meta": { "command": path, "duration": format_duration(start) }
                    });
                    if let Some(cta) = &cta {
                        err["meta"]["cta"] = serde_json::to_value(cta).unwrap_or_default();
                    }
                    writeln_fn(&stdout, &formatter::format(&err, fmt));
                }
                exit(1);
            }
        }
    }

    fn show_root_help(&self, stdout: &dyn Fn(&str)) {
        if let Some(root_cmd) = &self.root_command {
            let subcommands = collect_help_commands(&self.commands, &self.order);
            let help_text = help::format_command(
                &self.name,
                &FormatCommandOptions {
                    description: root_cmd
                        .description
                        .clone()
                        .or_else(|| self.description.clone()),
                    version: self.version.clone(),
                    args: root_cmd.args.clone(),
                    options: root_cmd.options.clone(),
                    examples: Vec::new(),
                    hint: root_cmd.hint.clone(),
                    subcommands,
                    root: true,
                },
            );
            writeln_fn(stdout, &help_text);
        } else {
            let cmds = collect_help_commands(&self.commands, &self.order);
            let help_text = help::format_root(
                &self.name,
                &FormatRootOptions {
                    description: self.description.clone(),
                    version: self.version.clone(),
                    commands: cmds,
                    root: true,
                },
            );
            writeln_fn(stdout, &help_text);
        }
    }

    fn resolve(&self, tokens: &[String]) -> Resolved<'_> {
        if tokens.is_empty() {
            return Resolved::Root { rest: Vec::new() };
        }

        let first = &tokens[0];
        let rest = &tokens[1..];

        if let Some(entry) = self.commands.get(first) {
            match entry {
                Entry::Command(cmd) => Resolved::Command {
                    cmd,
                    path: first.clone(),
                    rest: rest.to_vec(),
                },
                Entry::Group(group) => resolve_group(group, first, rest),
            }
        } else {
            Resolved::NotFound {
                name: first.clone(),
                path: String::new(),
            }
        }
    }

    fn build_manifest(&self) -> Value {
        let mut cmds = Vec::new();
        collect_manifest(&self.commands, &self.order, &[], &mut cmds);
        serde_json::json!({ "name": self.name, "commands": cmds })
    }

    fn format_llms_md(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("# {}", self.name));
        if let Some(desc) = &self.description {
            lines.push(String::new());
            lines.push(desc.clone());
        }
        lines.push(String::new());
        lines.push("## Commands".into());
        collect_llms_md(&self.commands, &self.order, &[], &self.name, &mut lines);
        lines.join("\n")
    }

    /// Collects all leaf commands as MCP tool entries for the MCP server.
    pub(crate) fn collect_mcp_tools(self) -> (String, String, Vec<mcp::ToolEntry>) {
        let name = self.name.clone();
        let version = self.version.clone().unwrap_or_else(|| "0.0.0".into());
        let mut tools = Vec::new();

        // Include root command if present
        if let Some(root_cmd) = self.root_command {
            let run: Arc<RunFn> = Arc::from(root_cmd.run);
            let input_schema = mcp::build_input_schema(&root_cmd.args, &root_cmd.options);
            let tool = mcp::make_tool(
                name.clone(),
                root_cmd.description.or_else(|| self.description.clone()),
                input_schema,
            );
            tools.push(mcp::ToolEntry {
                tool,
                args: root_cmd.args,
                options: root_cmd.options,
                run,
            });
        }

        collect_mcp_entries(self.commands, &self.order, &[], &mut tools);

        (name, version, tools)
    }
}

enum Resolved<'a> {
    Command {
        cmd: &'a Command,
        path: String,
        rest: Vec<String>,
    },
    Group {
        path: String,
        description: Option<String>,
        entries: &'a HashMap<String, Entry>,
        order: &'a Vec<String>,
    },
    NotFound {
        name: String,
        path: String,
    },
    Root {
        rest: Vec<String>,
    },
}

fn resolve_group<'a>(group: &'a Group, prefix: &str, tokens: &[String]) -> Resolved<'a> {
    if tokens.is_empty() {
        return Resolved::Group {
            path: prefix.to_string(),
            description: group.description.clone(),
            entries: &group.commands,
            order: &group.order,
        };
    }

    let next = &tokens[0];
    let rest = &tokens[1..];

    if let Some(entry) = group.commands.get(next) {
        match entry {
            Entry::Command(cmd) => Resolved::Command {
                cmd,
                path: format!("{prefix} {next}"),
                rest: rest.to_vec(),
            },
            Entry::Group(sub) => resolve_group(sub, &format!("{prefix} {next}"), rest),
        }
    } else {
        Resolved::NotFound {
            name: next.clone(),
            path: prefix.to_string(),
        }
    }
}

fn collect_help_commands(entries: &HashMap<String, Entry>, order: &[String]) -> Vec<CommandEntry> {
    order
        .iter()
        .filter_map(|name| {
            let entry = entries.get(name)?;
            let desc = match entry {
                Entry::Command(cmd) => cmd.description.clone(),
                Entry::Group(group) => group.description.clone(),
            };
            Some(CommandEntry {
                name: name.clone(),
                description: desc,
            })
        })
        .collect()
}

fn collect_manifest(
    entries: &HashMap<String, Entry>,
    order: &[String],
    prefix: &[&str],
    out: &mut Vec<Value>,
) {
    for name in order {
        if let Some(entry) = entries.get(name) {
            let path: Vec<&str> = prefix
                .iter()
                .copied()
                .chain(std::iter::once(name.as_str()))
                .collect();
            match entry {
                Entry::Command(cmd) => {
                    let mut obj = serde_json::json!({
                        "name": path.join(" "),
                    });
                    if let Some(desc) = &cmd.description {
                        obj["description"] = Value::String(desc.clone());
                    }
                    out.push(obj);
                }
                Entry::Group(group) => {
                    collect_manifest(&group.commands, &group.order, &path, out);
                }
            }
        }
    }
}

fn collect_llms_md(
    entries: &HashMap<String, Entry>,
    order: &[String],
    prefix: &[&str],
    cli_name: &str,
    lines: &mut Vec<String>,
) {
    for name in order {
        if let Some(entry) = entries.get(name) {
            let path: Vec<&str> = prefix
                .iter()
                .copied()
                .chain(std::iter::once(name.as_str()))
                .collect();
            match entry {
                Entry::Command(cmd) => {
                    let full_name = format!("{cli_name} {}", path.join(" "));
                    lines.push(String::new());
                    if let Some(desc) = &cmd.description {
                        lines.push(format!("### `{full_name}` — {desc}"));
                    } else {
                        lines.push(format!("### `{full_name}`"));
                    }
                    if !cmd.args.is_empty() {
                        lines.push(String::new());
                        lines.push("**Arguments:**".into());
                        for arg in &cmd.args {
                            let req = if arg.required { " (required)" } else { "" };
                            lines.push(format!("- `{}` — {}{req}", arg.name, arg.description));
                        }
                    }
                    if !cmd.options.is_empty() {
                        lines.push(String::new());
                        lines.push("**Options:**".into());
                        for opt in &cmd.options {
                            let kebab = parser::to_kebab(&opt.name);
                            lines.push(format!("- `--{kebab}` — {}", opt.description));
                        }
                    }
                }
                Entry::Group(group) => {
                    collect_llms_md(&group.commands, &group.order, &path, cli_name, lines);
                }
            }
        }
    }
}

fn collect_mcp_entries(
    entries: HashMap<String, Entry>,
    order: &[String],
    prefix: &[String],
    tools: &mut Vec<mcp::ToolEntry>,
) {
    // We need to consume the map, but iterate in `order`.
    // Drain the map into a temporary, then pull entries by order.
    let mut map = entries;
    for name in order {
        if let Some(entry) = map.remove(name) {
            let mut path = prefix.to_vec();
            path.push(name.clone());
            match entry {
                Entry::Command(cmd) => {
                    let tool_name = path.join("_");
                    let input_schema = mcp::build_input_schema(&cmd.args, &cmd.options);
                    let tool = mcp::make_tool(tool_name, cmd.description, input_schema);
                    tools.push(mcp::ToolEntry {
                        tool,
                        args: cmd.args,
                        options: cmd.options,
                        run: Arc::from(cmd.run),
                    });
                }
                Entry::Group(group) => {
                    collect_mcp_entries(group.commands, &group.order, &path, tools);
                }
            }
        }
    }
}

/// Builder for individual commands.
pub struct CommandBuilder {
    description: Option<String>,
    args: Vec<Arg>,
    options: Vec<Opt>,
    examples: Vec<ExampleEntry>,
    hint: Option<String>,
    format: Option<Format>,
    run: Option<RunFn>,
}

impl CommandBuilder {
    pub fn new() -> Self {
        Self {
            description: None,
            args: Vec::new(),
            options: Vec::new(),
            examples: Vec::new(),
            hint: None,
            format: None,
            run: None,
        }
    }

    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    pub fn arg(mut self, arg: Arg) -> Self {
        self.args.push(arg);
        self
    }

    pub fn option(mut self, opt: Opt) -> Self {
        self.options.push(opt);
        self
    }

    pub fn example(mut self, command: impl Into<String>, description: impl Into<String>) -> Self {
        self.examples.push(ExampleEntry {
            command: command.into(),
            description: Some(description.into()),
        });
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn format(mut self, fmt: Format) -> Self {
        self.format = Some(fmt);
        self
    }

    pub fn run(
        mut self,
        f: impl Fn(CommandContext) -> CommandResult + Send + Sync + 'static,
    ) -> Self {
        self.run = Some(Box::new(f));
        self
    }
}

impl Default for CommandBuilder {
    fn default() -> Self {
        Self::new()
    }
}

struct BuiltinFlags {
    verbose: bool,
    format: Format,
    format_explicit: bool,
    llms: bool,
    mcp: bool,
    help: bool,
    version: bool,
    rest: Vec<String>,
}

fn extract_builtin_flags(argv: &[String]) -> BuiltinFlags {
    let mut verbose = false;
    let mut llms = false;
    let mut mcp = false;
    let mut help = false;
    let mut version = false;
    let mut format = Format::Toon;
    let mut format_explicit = false;
    let mut rest = Vec::new();

    let mut i = 0;
    while i < argv.len() {
        let token = &argv[i];
        match token.as_str() {
            "--verbose" => verbose = true,
            "--llms" => llms = true,
            "--mcp" => mcp = true,
            "--help" | "-h" => help = true,
            "--version" => version = true,
            "--json" => {
                format = Format::Json;
                format_explicit = true;
            }
            "--format" => {
                if let Some(next) = argv.get(i + 1) {
                    if let Some(f) = Format::from_str(next) {
                        format = f;
                        format_explicit = true;
                    }
                    i += 1;
                }
            }
            _ => rest.push(token.clone()),
        }
        i += 1;
    }

    BuiltinFlags {
        verbose,
        format,
        format_explicit,
        llms,
        mcp,
        help,
        version,
        rest,
    }
}

struct ServeOptions {
    stdout: Box<dyn Fn(&str) + Send + Sync>,
    exit: Box<dyn Fn(i32) + Send + Sync>,
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self {
            stdout: Box::new(|s| {
                print!("{s}");
            }),
            exit: Box::new(|code| {
                std::process::exit(code);
            }),
        }
    }
}

fn writeln_fn(stdout: &dyn Fn(&str), s: &str) {
    if s.ends_with('\n') {
        stdout(s);
    } else {
        stdout(&format!("{s}\n"));
    }
}

fn format_duration(start: Instant) -> String {
    let elapsed = start.elapsed();
    format!("{}ms", elapsed.as_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn capture_cli(cli: Cli, argv: Vec<&str>) -> String {
        let output = Arc::new(Mutex::new(String::new()));
        let out_clone = output.clone();

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            cli.serve_with(
                argv.into_iter().map(String::from).collect(),
                ServeOptions {
                    stdout: Box::new(move |s| {
                        out_clone.lock().unwrap().push_str(s);
                    }),
                    exit: Box::new(|_| {}),
                },
            )
            .await;
        });

        Arc::try_unwrap(output).unwrap().into_inner().unwrap()
    }

    #[test]
    fn single_command_cli() {
        let cli = Cli::create("greet")
            .description("A greeting CLI")
            .arg(Arg::new("name").description("Name to greet").required(true))
            .run(|ctx| {
                let name = ctx.arg_str("name");
                CommandContext::ok(serde_json::json!({ "message": format!("hello {name}") }))
            });

        let out = capture_cli(cli, vec!["world"]);
        assert!(out.contains("hello world"));
    }

    #[test]
    fn multi_command_cli() {
        let cli = Cli::create("my-cli")
            .description("My CLI")
            .command(
                "status",
                CommandBuilder::new()
                    .description("Show status")
                    .run(|_| CommandContext::ok(serde_json::json!({ "clean": true }))),
            )
            .command(
                "install",
                CommandBuilder::new()
                    .description("Install a package")
                    .arg(Arg::new("package").description("Package name"))
                    .option(
                        Opt::new("saveDev")
                            .short('D')
                            .boolean()
                            .description("Save as dev dep"),
                    )
                    .run(|_| CommandContext::ok(serde_json::json!({ "added": 1 }))),
            );

        let out = capture_cli(cli, vec!["status"]);
        assert!(out.contains("clean: true"));
    }

    #[test]
    fn help_output() {
        let cli = Cli::create("my-cli")
            .description("My CLI")
            .version("1.0.0")
            .command(
                "status",
                CommandBuilder::new()
                    .description("Show status")
                    .run(|_| CommandContext::ok(Value::Null)),
            );

        let out = capture_cli(cli, vec!["--help"]);
        assert!(out.contains("my-cli \u{2014} My CLI"));
        assert!(out.contains("v1.0.0"));
        assert!(out.contains("status"));
    }

    #[test]
    fn json_format_flag() {
        let cli = Cli::create("test")
            .arg(Arg::new("name").required(true))
            .run(|ctx| CommandContext::ok(serde_json::json!({ "name": ctx.arg_str("name") })));

        let out = capture_cli(cli, vec!["--json", "alice"]);
        assert!(out.contains("\"name\": \"alice\""));
    }

    #[test]
    fn version_flag() {
        let cli = Cli::create("test").version("2.0.0");
        let out = capture_cli(cli, vec!["--version"]);
        assert!(out.contains("2.0.0"));
    }

    #[test]
    fn sub_cli_group() {
        let pr = Cli::create("pr")
            .description("Pull request commands")
            .command(
                "list",
                CommandBuilder::new()
                    .description("List PRs")
                    .option(
                        Opt::new("state")
                            .enum_values(["open", "closed", "all"])
                            .default_value("open"),
                    )
                    .run(|ctx| {
                        CommandContext::ok(serde_json::json!({ "state": ctx.option_str("state") }))
                    }),
            );

        let cli = Cli::create("my-cli").description("My CLI").group(pr);

        let out = capture_cli(cli, vec!["pr", "list", "--state", "closed"]);
        assert!(out.contains("state: closed"));
    }
}
