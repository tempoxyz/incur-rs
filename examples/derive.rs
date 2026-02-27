use incur::{CommandContext, Incur, IncurCommand};

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
}

#[tokio::main]
async fn main() {
    Deploy::cli()
        .run(|ctx| {
            let cmd = Deploy::from_context(&ctx);
            CommandContext::ok(serde_json::json!({
                "deployed": true,
                "env": cmd.env,
                "force": cmd.force,
                "timeout": cmd.timeout,
            }))
        })
        .serve()
        .await;
}
