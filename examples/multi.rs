use incur::{Arg, Cli, CommandBuilder, CommandContext, Opt};

#[tokio::main]
async fn main() {
    // Sub-command group
    let pr = Cli::create("pr")
        .description("Pull request commands")
        .command(
            "list",
            CommandBuilder::new()
                .description("List pull requests")
                .option(
                    Opt::new("state")
                        .description("Filter by state")
                        .enum_values(["open", "closed", "all"])
                        .default_value("open"),
                )
                .run(|ctx| {
                    CommandContext::ok(serde_json::json!({
                        "prs": [],
                        "state": ctx.option_str("state")
                    }))
                }),
        )
        .command(
            "create",
            CommandBuilder::new()
                .description("Create a pull request")
                .arg(Arg::new("title").description("PR title").required(true))
                .option(Opt::new("draft").boolean().description("Create as draft"))
                .run(|ctx| {
                    CommandContext::ok(serde_json::json!({
                        "number": 42,
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
                .description("Show repo status")
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
                        .description("Save as dev dependency"),
                )
                .run(|_| {
                    CommandContext::ok(serde_json::json!({ "added": 1, "packages": 451 }))
                }),
        )
        .group(pr)
        .serve()
        .await;
}
