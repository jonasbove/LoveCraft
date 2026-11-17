//! What the app is and where it comes from, for the About window, `--version` and diagnostics.

/// The name people see. A working name until the branding is decided.
pub const APP_NAME: &str = "prosecraft-rs";

/// This build's version, from the workspace manifest.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The project this app is a rewrite of.
pub const ORIGINAL_NAME: &str = "ProseCraft";

/// Who made the original, as its licence notice states.
pub const ORIGINAL_AUTHOR: &str = "Jennifer Roylance and contributors";

/// Where the original lives.
pub const ORIGINAL_URL: &str = "https://github.com/jennraeross/ProseCraft";

/// The upstream commit the port started from (ProseCraft v0.6.0).
pub const UPSTREAM_BASELINE: &str = "7dae544bfaa1e077b03d1b8a5dfe38a190ff8fb5";

/// One line of credit, shown in the About window and the README.
pub fn credit_line() -> String {
    format!(
        "{APP_NAME} is a Rust rewrite of {ORIGINAL_NAME} by {ORIGINAL_AUTHOR} ({ORIGINAL_URL}), used under the MIT licence."
    )
}

/// `prosecraft-rs 0.1.0`.
pub fn name_and_version() -> String {
    format!("{APP_NAME} {VERSION}")
}

#[cfg(test)]
mod tests {
    use super::{UPSTREAM_BASELINE, credit_line, name_and_version};

    #[test]
    fn credit_names_the_original_and_its_licence() {
        let line = credit_line();
        assert!(line.contains("ProseCraft"));
        assert!(line.contains("Jennifer Roylance"));
        assert!(line.contains("MIT"));
    }

    #[test]
    fn version_line_has_name_and_semver() {
        let line = name_and_version();
        let version = line.strip_prefix("prosecraft-rs ").unwrap_or_default();
        assert_eq!(version.split('.').count(), 3, "{line}");
    }

    #[test]
    fn baseline_is_a_full_commit_id() {
        assert_eq!(UPSTREAM_BASELINE.len(), 40);
        assert!(UPSTREAM_BASELINE.bytes().all(|b| b.is_ascii_hexdigit()));
    }
}
