#![allow(dead_code)]

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
    // Verify it can be further configured without panicking.
    let _cli = cli.run(|ctx| {
        let _cmd = Deploy::from_context(&ctx);
        CommandContext::ok(serde_json::json!({"ok": true}))
    });
}

#[test]
fn derive_command_builder() {
    let builder = Deploy::command_builder();
    // Can be used as a subcommand.
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
    use std::sync::{Arc, Mutex};

    let cli = Deploy::cli().run(|ctx| {
        let cmd = Deploy::from_context(&ctx);
        CommandContext::ok(serde_json::json!({
            "env": cmd.env,
            "force": cmd.force,
            "timeout": cmd.timeout,
        }))
    });

    let output = Arc::new(Mutex::new(String::new()));
    let out = output.clone();

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        cli.serve_argv(vec!["production".into(), "--force".into(), "--json".into()])
            .await;
    });

    // serve_argv writes to stdout directly, so we just verify no panic.
    // The real integration test is the example.
    let _ = out;
}
