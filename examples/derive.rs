use incur::{CommandContext, CommandResult, Incur, IncurRun};

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

impl IncurRun for Deploy {
    fn run(self) -> CommandResult {
        CommandContext::ok(serde_json::json!({
            "deployed": true,
            "env": self.env,
            "force": self.force,
            "timeout": self.timeout,
        }))
    }
}

#[tokio::main]
async fn main() {
    Deploy::serve().await;
}
