use std::borrow::Cow;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt, model::*,
    service::RequestContext, transport::stdio,
};
use serde_json::Value;

use crate::cli::{CommandContext, CommandResult, RunFn};
use crate::parser::{self, Arg, Opt, OptType};

/// Metadata for a single MCP tool backed by an incur command.
pub struct ToolEntry {
    pub tool: Tool,
    pub args: Vec<Arg>,
    pub options: Vec<Opt>,
    pub run: Arc<RunFn>,
}

/// MCP server that exposes incur CLI commands as tools.
pub(crate) struct IncurMcpServer {
    name: String,
    version: String,
    tools: Vec<ToolEntry>,
}

impl IncurMcpServer {
    pub fn new(name: String, version: String, tools: Vec<ToolEntry>) -> Self {
        Self {
            name,
            version,
            tools,
        }
    }
}

impl ServerHandler for IncurMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(format!("{} MCP server", self.name)),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: self.name.clone(),
                version: self.version.clone(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        let tools = self.tools.iter().map(|t| t.tool.clone()).collect();
        async move {
            Ok(ListToolsResult {
                tools,
                next_cursor: None,
                meta: None,
            })
        }
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let tool_name = request.name.as_ref();
        let entry = self
            .tools
            .iter()
            .find(|t| t.tool.name.as_ref() == tool_name)
            .ok_or_else(|| McpError::invalid_params(format!("unknown tool: {tool_name}"), None))?;

        let arguments = request.arguments.unwrap_or_default();

        // Split flat JSON params back into positional args and named options
        let mut argv: Vec<String> = Vec::new();

        // Positional args: extract in order from the schema
        for arg in &entry.args {
            if let Some(val) = arguments.get(&arg.name) {
                argv.push(json_value_to_string(val));
            }
        }

        // Named options: convert to --flag value pairs
        for opt in &entry.options {
            if let Some(val) = arguments.get(&opt.name) {
                let kebab = parser::to_kebab(&opt.name);
                match opt.opt_type {
                    OptType::Bool => {
                        if val.as_bool().unwrap_or(false) {
                            argv.push(format!("--{kebab}"));
                        }
                    }
                    OptType::Array => {
                        if let Some(arr) = val.as_array() {
                            for item in arr {
                                argv.push(format!("--{kebab}"));
                                argv.push(json_value_to_string(item));
                            }
                        }
                    }
                    _ => {
                        argv.push(format!("--{kebab}"));
                        argv.push(json_value_to_string(val));
                    }
                }
            }
        }

        // Parse using the existing parser
        let parsed = parser::parse(&argv, &entry.args, &entry.options)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

        let ctx = CommandContext::new(parsed, HashMap::new());
        let result = (entry.run)(ctx);

        match result {
            CommandResult::Ok(data) => Ok(CallToolResult {
                content: vec![Content::text(
                    serde_json::to_string(&data).unwrap_or_default(),
                )],
                structured_content: None,
                is_error: Some(false),
                meta: None,
            }),
            CommandResult::OkWith { data, .. } => Ok(CallToolResult {
                content: vec![Content::text(
                    serde_json::to_string(&data).unwrap_or_default(),
                )],
                structured_content: None,
                is_error: Some(false),
                meta: None,
            }),
            CommandResult::Err { code, message, .. } => Ok(CallToolResult {
                content: vec![Content::text(
                    serde_json::to_string(&serde_json::json!({
                        "error": { "code": code, "message": message }
                    }))
                    .unwrap_or_default(),
                )],
                structured_content: None,
                is_error: Some(true),
                meta: None,
            }),
        }
    }
}

/// Starts the MCP stdio server.
pub(crate) async fn serve(
    name: String,
    version: String,
    tools: Vec<ToolEntry>,
) -> Result<(), Box<dyn std::error::Error>> {
    let server = IncurMcpServer::new(name, version, tools);
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

/// Builds the JSON Schema `input_schema` for a command's args + options.
pub(crate) fn build_input_schema(args: &[Arg], options: &[Opt]) -> Arc<JsonObject> {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();

    for arg in args {
        let mut prop = serde_json::Map::new();
        prop.insert("type".into(), Value::String("string".into()));
        if !arg.description.is_empty() {
            prop.insert("description".into(), Value::String(arg.description.clone()));
        }
        properties.insert(arg.name.clone(), Value::Object(prop));
        if arg.required {
            required.push(Value::String(arg.name.clone()));
        }
    }

    for opt in options {
        let mut prop = serde_json::Map::new();
        match opt.opt_type {
            OptType::String => {
                prop.insert("type".into(), Value::String("string".into()));
            }
            OptType::Number => {
                prop.insert("type".into(), Value::String("number".into()));
            }
            OptType::Bool => {
                prop.insert("type".into(), Value::String("boolean".into()));
            }
            OptType::Array => {
                prop.insert("type".into(), Value::String("array".into()));
                let mut items = serde_json::Map::new();
                items.insert("type".into(), Value::String("string".into()));
                prop.insert("items".into(), Value::Object(items));
            }
        }
        if !opt.description.is_empty() {
            prop.insert("description".into(), Value::String(opt.description.clone()));
        }
        if !opt.enum_values.is_empty() {
            prop.insert(
                "enum".into(),
                Value::Array(
                    opt.enum_values
                        .iter()
                        .map(|v| Value::String(v.clone()))
                        .collect(),
                ),
            );
        }
        if let Some(default) = &opt.default {
            prop.insert("default".into(), Value::String(default.clone()));
        }
        properties.insert(opt.name.clone(), Value::Object(prop));
        if opt.required {
            required.push(Value::String(opt.name.clone()));
        }
    }

    let mut schema = serde_json::Map::new();
    schema.insert("type".into(), Value::String("object".into()));
    schema.insert("properties".into(), Value::Object(properties));
    if !required.is_empty() {
        schema.insert("required".into(), Value::Array(required));
    }

    Arc::new(schema)
}

/// Creates a `Tool` definition from a name, description, and schema.
pub(crate) fn make_tool(
    name: String,
    description: Option<String>,
    input_schema: Arc<JsonObject>,
) -> Tool {
    Tool {
        name: Cow::Owned(name),
        title: None,
        description: description.map(Cow::Owned),
        input_schema,
        output_schema: None,
        annotations: None,
        execution: None,
        icons: None,
        meta: None,
    }
}

fn json_value_to_string(val: &Value) -> String {
    match val {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cli, CommandBuilder};

    fn build_test_cli() -> Cli {
        let pr = Cli::create("pr")
            .description("Pull request commands")
            .command(
                "list",
                CommandBuilder::new()
                    .description("List PRs")
                    .option(
                        Opt::new("state")
                            .description("Filter by state")
                            .enum_values(["open", "closed", "all"])
                            .default_value("open"),
                    )
                    .run(|ctx| {
                        CommandContext::ok(serde_json::json!({ "state": ctx.option_str("state") }))
                    }),
            )
            .command(
                "create",
                CommandBuilder::new()
                    .description("Create a PR")
                    .arg(Arg::new("title").description("PR title").required(true))
                    .option(Opt::new("draft").boolean().description("Create as draft"))
                    .run(|ctx| {
                        CommandContext::ok(serde_json::json!({
                            "title": ctx.arg_str("title"),
                            "draft": ctx.option_bool("draft"),
                        }))
                    }),
            );

        Cli::create("my-cli")
            .description("My CLI")
            .version("1.0.0")
            .command(
                "status",
                CommandBuilder::new()
                    .description("Show status")
                    .run(|_| CommandContext::ok(serde_json::json!({ "clean": true }))),
            )
            .group(pr)
    }

    #[test]
    fn collects_tools_from_cli() {
        let cli = build_test_cli();
        let (name, version, tools) = cli.collect_mcp_tools();

        assert_eq!(name, "my-cli");
        assert_eq!(version, "1.0.0");

        let tool_names: Vec<&str> = tools.iter().map(|t| t.tool.name.as_ref()).collect();
        assert_eq!(tool_names.len(), 3);
        assert!(tool_names.contains(&"status"));
        assert!(tool_names.contains(&"pr_list"));
        assert!(tool_names.contains(&"pr_create"));
    }

    #[test]
    fn tool_descriptions() {
        let cli = build_test_cli();
        let (_, _, tools) = cli.collect_mcp_tools();

        let status = tools.iter().find(|t| &*t.tool.name == "status").unwrap();
        assert_eq!(status.tool.description.as_deref(), Some("Show status"));

        let pr_create = tools.iter().find(|t| &*t.tool.name == "pr_create").unwrap();
        assert_eq!(pr_create.tool.description.as_deref(), Some("Create a PR"));
    }

    #[test]
    fn input_schema_for_args_and_options() {
        let schema = build_input_schema(
            &[Arg::new("name").description("The name").required(true)],
            &[
                Opt::new("verbose").boolean().description("Be verbose"),
                Opt::new("count").number(),
            ],
        );

        let schema_val = Value::Object((*schema).clone());
        assert_eq!(schema_val["type"], "object");

        let props = schema_val["properties"].as_object().unwrap();
        assert_eq!(props["name"]["type"], "string");
        assert_eq!(props["name"]["description"], "The name");
        assert_eq!(props["verbose"]["type"], "boolean");
        assert_eq!(props["count"]["type"], "number");

        let required = schema_val["required"].as_array().unwrap();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], "name");
    }

    #[test]
    fn input_schema_enum_values() {
        let schema = build_input_schema(
            &[],
            &[Opt::new("state")
                .enum_values(["open", "closed"])
                .default_value("open")],
        );

        let schema_val = Value::Object((*schema).clone());
        let state = &schema_val["properties"]["state"];
        assert_eq!(state["enum"], serde_json::json!(["open", "closed"]));
        assert_eq!(state["default"], "open");
    }

    #[test]
    fn execute_tool_directly() {
        let cli = build_test_cli();
        let (_, _, tools) = cli.collect_mcp_tools();

        // Find and execute the "status" tool
        let status = tools.iter().find(|t| &*t.tool.name == "status").unwrap();
        let parsed = parser::parse(&[], &status.args, &status.options).unwrap();
        let ctx = CommandContext::new(parsed, HashMap::new());
        let result = (status.run)(ctx);
        match result {
            CommandResult::Ok(data) => {
                assert_eq!(data, serde_json::json!({"clean": true}));
            }
            _ => panic!("expected Ok result"),
        }
    }

    #[test]
    fn execute_tool_with_args() {
        let cli = build_test_cli();
        let (_, _, tools) = cli.collect_mcp_tools();

        // Find and execute "pr_create" with arguments
        let pr_create = tools.iter().find(|t| &*t.tool.name == "pr_create").unwrap();
        let argv: Vec<String> = vec!["my pr".into(), "--draft".into()];
        let parsed = parser::parse(&argv, &pr_create.args, &pr_create.options).unwrap();
        let ctx = CommandContext::new(parsed, HashMap::new());
        let result = (pr_create.run)(ctx);
        match result {
            CommandResult::Ok(data) => {
                assert_eq!(data["title"], "my pr");
                assert_eq!(data["draft"], true);
            }
            _ => panic!("expected Ok result"),
        }
    }

    #[test]
    fn json_params_to_argv() {
        let cli = build_test_cli();
        let (_, _, tools) = cli.collect_mcp_tools();

        // Simulate MCP call_tool: flat JSON params → argv → parse → execute
        let pr_create = tools.iter().find(|t| &*t.tool.name == "pr_create").unwrap();

        let mut arguments = serde_json::Map::new();
        arguments.insert("title".into(), Value::String("test pr".into()));
        arguments.insert("draft".into(), Value::Bool(true));

        // Reconstruct argv from JSON params (same logic as call_tool handler)
        let mut argv: Vec<String> = Vec::new();
        for arg in &pr_create.args {
            if let Some(val) = arguments.get(&arg.name) {
                argv.push(json_value_to_string(val));
            }
        }
        for opt in &pr_create.options {
            if let Some(val) = arguments.get(&opt.name) {
                let kebab = parser::to_kebab(&opt.name);
                if opt.opt_type == OptType::Bool {
                    if val.as_bool().unwrap_or(false) {
                        argv.push(format!("--{kebab}"));
                    }
                } else {
                    argv.push(format!("--{kebab}"));
                    argv.push(json_value_to_string(val));
                }
            }
        }

        let parsed = parser::parse(&argv, &pr_create.args, &pr_create.options).unwrap();
        let ctx = CommandContext::new(parsed, HashMap::new());
        let result = (pr_create.run)(ctx);
        match result {
            CommandResult::Ok(data) => {
                assert_eq!(data["title"], "test pr");
                assert_eq!(data["draft"], true);
            }
            _ => panic!("expected Ok result"),
        }
    }

    #[test]
    fn server_info() {
        let server = IncurMcpServer::new("test".into(), "1.0.0".into(), vec![]);
        let info = server.get_info();
        assert_eq!(&*info.server_info.name, "test");
        assert_eq!(&*info.server_info.version, "1.0.0");
    }

    #[test]
    fn root_command_becomes_tool() {
        let cli = Cli::create("greet")
            .description("A greeting CLI")
            .version("1.0.0")
            .arg(Arg::new("name").description("Name to greet").required(true))
            .run(|ctx| {
                CommandContext::ok(
                    serde_json::json!({ "message": format!("hello {}", ctx.arg_str("name")) }),
                )
            });

        let (name, _, tools) = cli.collect_mcp_tools();
        assert_eq!(name, "greet");
        let tool_names: Vec<&str> = tools.iter().map(|t| t.tool.name.as_ref()).collect();
        assert_eq!(tool_names.len(), 1);
        assert_eq!(tool_names[0], "greet");
        assert_eq!(tools[0].tool.description.as_deref(), Some("A greeting CLI"));
    }
}
