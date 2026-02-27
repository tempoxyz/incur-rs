use incur::{CommandContext, Incur, IncurCommand};

/// Example: config waterfall with derive.
///
/// Options resolve with this precedence (highest wins):
///   1. CLI args:      `deploy production --force --timeout 120`
///   2. Env vars:      `DEPLOY_TIMEOUT=90` or explicit `MY_API_KEY=sk-...`
///   3. TOML config:   `deploy.toml` (discovered walking up from cwd)
///   4. Code defaults: `#[incur(default = "30")]`
///
/// Try it:
///   # code default only
///   cargo run --example derive -- production
///
///   # with a config file
///   echo 'timeout = 60' > deploy.toml
///   cargo run --example derive -- production
///
///   # env var overrides config file
///   DEPLOY_TIMEOUT=90 cargo run --example derive -- production
///
///   # CLI arg overrides everything
///   DEPLOY_TIMEOUT=90 cargo run --example derive -- production --timeout 120
///
///   # explicit env var via #[incur(env = "...")]
///   MY_API_KEY=sk-secret cargo run --example derive -- production
#[derive(Incur)]
#[incur(name = "deploy", description = "Deploy the app", version = "1.0.0")]
struct Deploy {
    /// Environment to deploy to
    #[incur(arg, required)]
    env: String,

    /// Force deploy without confirmation
    #[incur(option, short = 'f')]
    force: bool,

    /// Timeout in seconds
    #[incur(option, default = "30")]
    timeout: f64,

    /// API key (read from MY_API_KEY env var)
    #[incur(option, env = "MY_API_KEY")]
    api_key: Option<String>,
}

#[tokio::main]
async fn main() {
    Deploy::cli()
        .config_file("deploy.toml")
        .env_prefix("DEPLOY")
        .run(|ctx| {
            let cmd = Deploy::from_context(&ctx);
            CommandContext::ok(serde_json::json!({
                "deployed": true,
                "env": cmd.env,
                "force": cmd.force,
                "timeout": cmd.timeout,
                "api_key": cmd.api_key.as_deref().unwrap_or("(not set)"),
            }))
        })
        .serve()
        .await;
}
