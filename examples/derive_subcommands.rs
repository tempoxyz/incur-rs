use incur::{Cli, CommandContext, CommandResult, Incur, IncurRun};

#[derive(Incur)]
#[incur(description = "Show repo status")]
struct Status {
    /// Show verbose output
    #[incur(option, short = 'v')]
    verbose: bool,
}

impl IncurRun for Status {
    fn run(self) -> CommandResult {
        CommandContext::ok(serde_json::json!({
            "clean": true,
            "verbose": self.verbose,
        }))
    }
}

#[derive(Incur)]
#[incur(description = "Install a package")]
struct Install {
    /// Package name
    #[incur(arg, required)]
    package: String,

    /// Save as dev dependency
    #[incur(option, short = 'D')]
    save_dev: bool,
}

impl IncurRun for Install {
    fn run(self) -> CommandResult {
        CommandContext::ok(serde_json::json!({
            "added": 1,
            "package": self.package,
            "dev": self.save_dev,
        }))
    }
}

// Sub-command group: pr list, pr create
#[derive(Incur)]
#[incur(name = "list", description = "List pull requests")]
struct PrList {
    /// Filter by state
    #[incur(option, enum_values = ["open", "closed", "all"], default = "open")]
    state: String,
}

impl IncurRun for PrList {
    fn run(self) -> CommandResult {
        CommandContext::ok(serde_json::json!({
            "prs": [],
            "state": self.state,
        }))
    }
}

#[derive(Incur)]
#[incur(name = "create", description = "Create a pull request")]
struct PrCreate {
    /// PR title
    #[incur(arg, required)]
    title: String,

    /// Create as draft
    #[incur(option)]
    draft: bool,
}

impl IncurRun for PrCreate {
    fn run(self) -> CommandResult {
        CommandContext::ok(serde_json::json!({
            "number": 42,
            "title": self.title,
            "draft": self.draft,
        }))
    }
}

#[tokio::main]
async fn main() {
    // Build a sub-CLI group for `pr` commands
    let pr = Cli::create("pr")
        .description("Pull request commands")
        .command_run::<PrList>()
        .command_run::<PrCreate>();

    // Build the root CLI with subcommands
    Cli::create("my-cli")
        .description("My CLI")
        .version("1.0.0")
        .command_run::<Status>()
        .command_run::<Install>()
        .group(pr)
        .serve()
        .await;
}
