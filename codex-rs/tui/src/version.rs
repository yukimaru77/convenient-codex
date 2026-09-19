/// The current Codex CLI version as embedded at compile time.
#[cfg(not(test))]
pub const CODEX_CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

// Keep upstream UI fixtures independent of the release version and its display width.
#[cfg(test)]
pub const CODEX_CLI_VERSION: &str = "0.0.0";
