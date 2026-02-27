//! incur — CLI framework for agents and humans.
//!
//! A Rust port of [wevm/incur](https://github.com/wevm/incur), designed as a
//! clap replacement with first-class support for AI agent discovery, TOON
//! output, call-to-actions, and structured I/O.
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use incur::{Cli, Arg, CommandContext};
//!
//! #[tokio::main]
//! async fn main() {
//!     Cli::create("greet")
//!         .description("A greeting CLI")
//!         .arg(Arg::new("name").description("Name to greet").required(true))
//!         .run(|ctx| {
//!             let name = ctx.arg::<String>("name");
//!             CommandContext::ok(serde_json::json!({ "message": format!("hello {name}") }))
//!         })
//!         .serve()
//!         .await;
//! }
//! ```

pub mod cli;
pub mod cta;
pub mod derive_support;
pub mod error;
pub mod formatter;
#[cfg(feature = "clap")]
pub mod from_clap;
pub mod help;
pub mod mcp;
pub mod parser;

pub use cli::{Cli, CommandBuilder, CommandContext, CommandResult, RunFn};
pub use cta::Cta;
pub use derive_support::IncurCommand;
pub use error::{IncurError, IncurResult};
pub use formatter::Format;
pub use incur_derive::Incur;
pub use parser::{Arg, Opt};
