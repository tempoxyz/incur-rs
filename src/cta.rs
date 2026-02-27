/// A call-to-action suggesting a next command.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Cta {
    /// The full command string.
    pub command: String,
    /// A short description of what the command does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Cta {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            description: None,
        }
    }

    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
}

/// A block of CTAs with a label.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CtaBlock {
    pub description: String,
    pub commands: Vec<Cta>,
}

impl CtaBlock {
    pub fn new(commands: Vec<Cta>) -> Self {
        Self {
            description: "Next".into(),
            commands,
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.description = label.into();
        self
    }
}

/// Formats a CTA block for human-readable output.
pub fn format_human(cli_name: &str, block: &CtaBlock) -> String {
    let mut lines = Vec::new();
    lines.push(format!("{}:", block.description));
    for cta in &block.commands {
        if let Some(desc) = &cta.description {
            lines.push(format!("  {cli_name} {} \u{2014} {desc}", cta.command));
        } else {
            lines.push(format!("  {cli_name} {}", cta.command));
        }
    }
    lines.join("\n")
}
