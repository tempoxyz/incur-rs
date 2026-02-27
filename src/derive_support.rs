use crate::cli::{Cli, CommandBuilder, CommandContext, CommandResult};
use crate::parser::{Arg, Opt};

/// Trait implemented by `#[derive(Incur)]` for structs that define CLI commands.
pub trait IncurCommand: Sized {
    /// Returns the positional arguments for this command.
    fn incur_args() -> Vec<Arg>;

    /// Returns the named options for this command.
    fn incur_options() -> Vec<Opt>;

    /// Constructs this struct from a parsed `CommandContext`.
    fn from_context(ctx: &CommandContext) -> Self;

    /// The CLI name.
    fn cli_name() -> &'static str;

    /// The CLI description, if specified.
    fn cli_description() -> Option<&'static str>;

    /// The CLI version, if specified.
    fn cli_version() -> Option<&'static str>;

    /// Builds a `Cli` configured with this command's args and options.
    /// Use `.run(...)` to attach a handler, or implement `IncurRun` and
    /// call `T::serve()` directly.
    fn cli() -> Cli {
        let mut cli = Cli::create(Self::cli_name());
        if let Some(desc) = Self::cli_description() {
            cli = cli.description(desc);
        }
        if let Some(ver) = Self::cli_version() {
            cli = cli.version(ver);
        }
        for arg in Self::incur_args() {
            cli = cli.arg(arg);
        }
        for opt in Self::incur_options() {
            cli = cli.option(opt);
        }
        cli
    }

    /// Builds a `CommandBuilder` for use as a subcommand.
    fn command_builder() -> CommandBuilder {
        let mut builder = CommandBuilder::new();
        if let Some(desc) = Self::cli_description() {
            builder = builder.description(desc);
        }
        for arg in Self::incur_args() {
            builder = builder.arg(arg);
        }
        for opt in Self::incur_options() {
            builder = builder.option(opt);
        }
        builder
    }
}

/// Implement this on a derived struct to define the command handler.
///
/// ```rust,ignore
/// #[derive(Incur)]
/// #[incur(name = "deploy", description = "Deploy the app")]
/// struct Deploy {
///     #[incur(arg, required)]
///     env: String,
///     #[incur(option, short = 'f')]
///     force: bool,
/// }
///
/// impl IncurRun for Deploy {
///     fn run(self) -> CommandResult {
///         CommandContext::ok(serde_json::json!({
///             "deployed": true,
///             "env": self.env,
///         }))
///     }
/// }
///
/// // Single command:
/// Deploy::serve().await;
///
/// // As subcommand:
/// Cli::create("app")
///     .command_run::<Deploy>()
///     .serve()
///     .await;
/// ```
pub trait IncurRun: IncurCommand + Send + Sync + 'static {
    fn run(self) -> CommandResult;

    /// Serves this command directly as a standalone CLI.
    fn serve() -> impl std::future::Future<Output = ()> + Send {
        async {
            Self::cli()
                .run(|ctx| Self::from_context(&ctx).run())
                .serve()
                .await;
        }
    }
}
