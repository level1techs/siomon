//! Command-line argument parsing and config-file application.

use crate::config::SiomonConfig;

pub use crate::opts::*;

impl Cli {
    /// Apply config file values for any CLI argument that wasn't explicitly set.
    pub fn apply_config(&mut self, config: &SiomonConfig, matches: &clap::ArgMatches) {
        if !self.is_explicitly_set("format", matches) {
            match config.general.format.as_str() {
                "json" => self.format = OutputFormat::Json,
                "xml" => self.format = OutputFormat::Xml,
                "html" => self.format = OutputFormat::Html,
                "text" => self.format = OutputFormat::Text,
                other => log::warn!("Unknown format in config: {other:?}"),
            }
        }

        if !self.is_explicitly_set("color", matches) {
            match config.general.color.as_str() {
                "auto" => self.color = ColorMode::Auto,
                "always" => self.color = ColorMode::Always,
                "never" => self.color = ColorMode::Never,
                other => log::warn!("Unknown color mode in config: {other:?}"),
            }
        }

        if !self.is_explicitly_set("interval", matches) {
            self.interval = config.general.poll_interval_ms;
        }

        if !self.is_explicitly_set("no_nvidia", matches) {
            self.no_nvidia = config.general.no_nvidia;
        }
    }

    /// Check if an argument was explicitly set on the command line (not just a default).
    /// Recursive to handle global arguments placed after subcommands.
    pub fn is_explicitly_set(&self, id: &str, matches: &clap::ArgMatches) -> bool {
        use clap::parser::ValueSource;

        if matches
            .value_source(id)
            .is_some_and(|s| s != ValueSource::DefaultValue)
        {
            return true;
        }

        if let Some((_, sub_m)) = matches.subcommand() {
            return self.is_explicitly_set(id, sub_m);
        }

        false
    }
}
