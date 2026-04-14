# incur

Rust CLI framework for agents and humans. Port of [wevm/incur](https://github.com/wevm/incur).

Built-in MCP server, config waterfall (CLI → env → TOML → defaults), structured output, and agent-friendly discovery.

## Usage

```rust
use incur::{Cli, Arg, CommandContext};

#[tokio::main]
async fn main() {
    Cli::create("greet")
        .description("A greeting CLI")
        .arg(Arg::new("name").description("Name to greet").required(true))
        .run(|ctx| {
            let name = ctx.arg::<String>("name");
            CommandContext::ok(serde_json::json!({ "message": format!("hello {name}") }))
        })
        .serve()
        .await;
}
```

Or with derive:

```rust
#[derive(Incur)]
#[incur(name = "deploy", description = "Deploy the app")]
struct Deploy {
    #[incur(arg, required)]
    env: String,

    #[incur(option, short = 'f')]
    force: bool,

    #[incur(option, default = "30")]
    timeout: f64,
}
```

## Features

- **MCP server** — every command is automatically an MCP tool (`--mcp` flag)
- **Config waterfall** — CLI args > env vars > TOML config > code defaults
- **Output formats** — JSON, YAML, Markdown, JSONL, TOON (`--format`)
- **Clap bridge** — wrap existing clap apps with `from_clap` (feature `clap`)

## License

MIT OR Apache-2.0
