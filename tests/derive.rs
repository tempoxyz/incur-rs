use incur::{Cli, CommandContext, CommandResult, Incur, IncurCommand, IncurRun};

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

#[derive(Incur)]
struct Minimal {
    #[incur(arg)]
    name: Option<String>,
}

#[derive(Incur)]
#[incur(name = "tagged")]
struct WithEnum {
    #[incur(option, enum_values = ["dev", "staging", "prod"])]
    env: String,
}

#[derive(Incur)]
#[incur(name = "arrays")]
struct WithArray {
    #[incur(option)]
    tags: Vec<String>,
}

#[test]
fn derive_args() {
    let args = Deploy::incur_args();
    assert_eq!(args.len(), 1);
    assert_eq!(args[0].name, "env");
    assert_eq!(args[0].description, "Environment to deploy to");
    assert!(args[0].required);
}

#[test]
fn derive_options() {
    let opts = Deploy::incur_options();
    assert_eq!(opts.len(), 2);

    assert_eq!(opts[0].name, "force");
    assert_eq!(opts[0].description, "Force deploy without confirmation");
    assert_eq!(opts[0].short, Some('f'));
    assert_eq!(opts[0].opt_type, incur::parser::OptType::Bool);

    assert_eq!(opts[1].name, "timeout");
    assert_eq!(opts[1].description, "Timeout in seconds");
    assert_eq!(opts[1].opt_type, incur::parser::OptType::Number);
    assert_eq!(opts[1].default.as_deref(), Some("30"));
}

#[test]
fn derive_cli_metadata() {
    assert_eq!(Deploy::cli_name(), "deploy");
    assert_eq!(Deploy::cli_description(), Some("Deploy the app"));
    assert_eq!(Deploy::cli_version(), Some("1.0.0"));
}

#[test]
fn derive_defaults_name_to_lowercase() {
    assert_eq!(Minimal::cli_name(), "minimal");
    assert_eq!(Minimal::cli_description(), None);
    assert_eq!(Minimal::cli_version(), None);
}

#[test]
fn derive_optional_arg() {
    let args = Minimal::incur_args();
    assert_eq!(args.len(), 1);
    assert!(!args[0].required);
}

#[test]
fn derive_enum_values() {
    let opts = WithEnum::incur_options();
    assert_eq!(opts.len(), 1);
    assert_eq!(opts[0].enum_values, vec!["dev", "staging", "prod"]);
}

#[test]
fn derive_array_option() {
    let opts = WithArray::incur_options();
    assert_eq!(opts.len(), 1);
    assert_eq!(opts[0].opt_type, incur::parser::OptType::Array);
}

#[test]
fn derive_cli_builds() {
    let cli = Deploy::cli();
    let _cli = cli.run(|ctx| {
        let _cmd = Deploy::from_context(&ctx);
        CommandContext::ok(serde_json::json!({"ok": true}))
    });
}

#[test]
fn derive_command_builder() {
    let builder = Deploy::command_builder();
    let _cli = incur::Cli::create("app").command(
        "deploy",
        builder.run(|ctx| {
            let _cmd = Deploy::from_context(&ctx);
            CommandContext::ok(serde_json::json!({"ok": true}))
        }),
    );
}

#[test]
fn derive_from_context_integration() {
    let cli = Deploy::cli().run(|ctx| {
        let cmd = Deploy::from_context(&ctx);
        CommandContext::ok(serde_json::json!({
            "env": cmd.env,
            "force": cmd.force,
            "timeout": cmd.timeout,
        }))
    });

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        cli.serve_argv(vec!["production".into(), "--force".into(), "--json".into()])
            .await;
    });
}

// --- IncurRun and subcommand tests ---

#[derive(Incur)]
#[incur(name = "status", description = "Show status")]
struct Status {
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

#[test]
fn command_run_registers_subcommand() {
    // command_run::<T>() should register a subcommand using cli_name()
    let cli = Cli::create("app")
        .description("Test app")
        .command_run::<Deploy>()
        .command_run::<Status>();

    let output = capture_cli(cli, vec!["--help"]);
    assert!(output.contains("deploy"));
    assert!(output.contains("status"));
}

#[test]
fn command_run_executes_handler() {
    let cli = Cli::create("app")
        .command_run::<Deploy>()
        .command_run::<Status>();

    let output = capture_cli(cli, vec!["deploy", "production", "--force", "--json"]);
    assert!(output.contains("\"deployed\": true"));
    assert!(output.contains("\"env\": \"production\""));
    assert!(output.contains("\"force\": true"));
}

#[test]
fn command_run_subcommand_group() {
    #[derive(Incur)]
    #[incur(name = "list", description = "List items")]
    struct List {
        #[incur(option, default = "open")]
        state: String,
    }

    impl IncurRun for List {
        fn run(self) -> CommandResult {
            CommandContext::ok(serde_json::json!({ "state": self.state }))
        }
    }

    let sub = Cli::create("pr")
        .description("PR commands")
        .command_run::<List>();

    let cli = Cli::create("app").group(sub);

    let output = capture_cli(cli, vec!["pr", "list", "--state", "closed", "--json"]);
    assert!(output.contains("\"state\": \"closed\""));
}

#[test]
fn incur_run_serve() {
    // Verify Deploy::serve() compiles and works (runs serve_argv internally)
    let cli = Deploy::cli().run(|ctx| Deploy::from_context(&ctx).run());
    let output = capture_cli(cli, vec!["staging", "--json"]);
    assert!(output.contains("\"deployed\": true"));
    assert!(output.contains("\"env\": \"staging\""));
}

// --- Test helpers ---

fn capture_cli(cli: incur::Cli, argv: Vec<&str>) -> String {
    use std::sync::{Arc, Mutex};

    let output = Arc::new(Mutex::new(String::new()));
    let out = output.clone();

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        cli.serve_with_test(argv.into_iter().map(String::from).collect(), move |s| {
            out.lock().unwrap().push_str(s);
        })
        .await;
    });

    Arc::try_unwrap(output).unwrap().into_inner().unwrap()
}
