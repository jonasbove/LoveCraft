//! What the app is and where it comes from, for the About window, `--version` and diagnostics.

/// The name people see. Until 0.2.0 it was ProseCraft-rs, which the ids in [`ids`] keep.
pub const APP_NAME: &str = "LoveCraft";

/// The ids and folder names that are stored on people's computers or that other programs find
/// the app by. They keep the app's former name, ProseCraft-rs: changing one would lose a library,
/// its settings or the window's state on update, or break a pairing with CraftCloud or an MCP
/// client. So they never follow [`APP_NAME`].
pub mod ids {
    /// The desktop app id: the Wayland app id and X11 class, the Linux desktop entry, AppStream
    /// and Flatpak id, and the Windows AppUserModelID.
    pub const APP_ID: &str = "io.github.jonasbove.prosecraft_rs";
    /// The library's folder under the platform's data folder.
    pub const DATA_DIR: &str = "prosecraft-rs";
    /// The variable that puts the library somewhere else.
    pub const DATA_DIR_ENV: &str = "PROSECRAFT_RS_DATA_DIR";
    /// The name eframe keeps the window's state under.
    pub const WINDOW_STATE_KEY: &str = "prosecraft-rs";
    /// The web app's IndexedDB database.
    pub const WEB_DATABASE: &str = "prosecraft-rs";
    /// The prefix of the web app's `localStorage` keys.
    pub const WEB_SETTING_PREFIX: &str = "prosecraft-rs:";
    /// The app's id in CraftCloud (`craftcloud files --app prosecraft-rs`, its `apps.toml`).
    pub const CRAFTCLOUD_APP_ID: &str = "prosecraft-rs";
    /// The prefix of the messages a Craft host page and the web app send each other
    /// (`prosecraft-rs:open`).
    pub const HOST_MESSAGE_PREFIX: &str = "prosecraft-rs";
    /// The MCP server's protocol name, ProseCraft's own server's (MCP clients may key on it).
    pub const MCP_SERVER_NAME: &str = "prosecraft";
}

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

/// `LoveCraft 0.2.0`.
pub fn name_and_version() -> String {
    format!("{APP_NAME} {VERSION}")
}

/// The licence the original and this rewrite are under (`APP_LICENSE`).
pub const LICENSE: &str = "MIT";

/// `summarizeUserAgent`: the operating system and the browser engine a user agent names, and
/// nothing else of it.
pub fn summarize_user_agent(user_agent: &str) -> String {
    let lower = user_agent.to_lowercase();
    let os = if ["windows nt", "win32", "win64"]
        .iter()
        .any(|k| lower.contains(k))
    {
        "Windows"
    } else if lower.contains("macintosh") || lower.contains("mac os") {
        "macOS"
    } else if lower.contains("linux") || lower.contains("x11") {
        "Linux"
    } else {
        "unknown"
    };
    // The digits after `token` (`/(\d+)/`).
    let version = |text: &str, token: &str| {
        let at = text.find(token)? + token.len();
        let digits: String = text[at..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        (!digits.is_empty()).then_some(digits)
    };
    // `/Edg[\s\/]/i`.
    let edge = lower.match_indices("edg").any(|(at, _)| {
        lower[at + 3..]
            .chars()
            .next()
            .is_some_and(|c| c == '/' || c.is_whitespace())
    }) || lower.contains("edgios");
    let engine = if edge {
        match version(&lower, "edg/") {
            Some(v) => format!("WebView2 (Chromium {v})"),
            None => "WebView2 (Chromium)".to_owned(),
        }
    } else if lower.contains("trident") || lower.contains("msie") {
        "Trident".to_owned()
    } else if let Some(v) = version(user_agent, "Chrome/") {
        format!("Chromium {v}")
    } else if lower.contains("webkit") {
        if os == "Linux" { "WebKitGTK" } else { "WebKit" }.to_owned()
    } else if lower.contains("blink") {
        "Blink".to_owned()
    } else {
        "unknown".to_owned()
    };
    format!("{os}, {engine}")
}

/// What the diagnostics report says (`DiagnosticsInput`): the setup and counts, never titles or
/// text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diagnostics {
    pub version: String,
    /// The operating system and engine (from [`summarize_user_agent`] on the web).
    pub os: String,
    pub storage_backend: String,
    pub online: Option<bool>,
    pub projects: usize,
    pub chapters: usize,
    pub scenes: usize,
    pub wiki_pages: usize,
    pub words: usize,
    pub comments: usize,
    pub snapshots: usize,
    pub features: Vec<(String, bool)>,
}

/// `buildDiagnosticsReport`, at `date` (an ISO 8601 time).
pub fn diagnostics_report(input: &Diagnostics, date: &str) -> String {
    let mut lines = vec![
        format!("app: {APP_NAME} {}", input.version),
        format!("date: {date}"),
        format!("os: {}", input.os),
        format!("storage backend: {}", input.storage_backend),
        format!(
            "online: {}",
            input.online.map_or("unknown".to_owned(), |o| o.to_string())
        ),
        format!("projects: {}", input.projects),
        format!("chapters: {}", input.chapters),
        format!("scenes: {}", input.scenes),
        format!("wiki pages: {}", input.wiki_pages),
        format!("words: {}", input.words),
        format!("comments: {}", input.comments),
        format!("snapshots: {}", input.snapshots),
    ];
    for (name, on) in &input.features {
        lines.push(format!("{name}: {}", if *on { "on" } else { "off" }));
    }
    lines.join("\n")
}

#[cfg(test)]
mod diagnostics_tests {
    use super::{Diagnostics, diagnostics_report, summarize_user_agent};

    #[test]
    fn user_agents_are_summarized_as_prosecraft_does() {
        let linux_webkit = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";
        assert_eq!(summarize_user_agent(linux_webkit), "Linux, WebKitGTK");
        let edge = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36 Edg/126.0.0.0";
        assert_eq!(
            summarize_user_agent(edge),
            "Windows, WebView2 (Chromium 126)"
        );
        let mac = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.6 Safari/605.1.16";
        assert_eq!(summarize_user_agent(mac), "macOS, WebKit");
        assert_eq!(
            summarize_user_agent("Some random user agent string"),
            "unknown, unknown"
        );
        let chrome = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";
        assert_eq!(summarize_user_agent(chrome), "Linux, Chromium 126");
        assert_eq!(
            summarize_user_agent("/home/alice/secret data here"),
            "unknown, unknown"
        );
    }

    #[test]
    fn the_report_lists_the_setup_and_counts_only() {
        let input = Diagnostics {
            version: "0.4.6".into(),
            os: "Linux, egui".into(),
            storage_backend: "IndexedDB".into(),
            online: Some(false),
            projects: 5,
            chapters: 10,
            scenes: 20,
            wiki_pages: 3,
            words: 15000,
            comments: 50,
            snapshots: 2,
            features: vec![("darkMode".into(), true), ("autosave".into(), false)],
        };
        let report = diagnostics_report(&input, "2026-01-02T03:04:05.000Z");
        assert_eq!(
            report,
            "app: LoveCraft 0.4.6\ndate: 2026-01-02T03:04:05.000Z\nos: Linux, egui\nstorage backend: IndexedDB\nonline: false\nprojects: 5\nchapters: 10\nscenes: 20\nwiki pages: 3\nwords: 15000\ncomments: 50\nsnapshots: 2\ndarkMode: on\nautosave: off"
        );
        let unknown = Diagnostics {
            online: None,
            ..input
        };
        assert!(diagnostics_report(&unknown, "d").contains("online: unknown"));
    }
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
        let version = line.strip_prefix("LoveCraft ").unwrap_or_default();
        assert_eq!(version.split('.').count(), 3, "{line}");
    }

    /// The rename to LoveCraft must not move anything a 0.1 install stored or paired with.
    #[test]
    fn persisted_ids_keep_the_former_name() {
        use super::ids;
        assert_eq!(super::APP_NAME, "LoveCraft");
        assert_eq!(ids::APP_ID, "io.github.jonasbove.prosecraft_rs");
        assert_eq!(ids::DATA_DIR, "prosecraft-rs");
        assert_eq!(ids::DATA_DIR_ENV, "PROSECRAFT_RS_DATA_DIR");
        assert_eq!(ids::WINDOW_STATE_KEY, "prosecraft-rs");
        assert_eq!(ids::WEB_DATABASE, "prosecraft-rs");
        assert_eq!(ids::WEB_SETTING_PREFIX, "prosecraft-rs:");
        assert_eq!(ids::CRAFTCLOUD_APP_ID, "prosecraft-rs");
        assert_eq!(ids::HOST_MESSAGE_PREFIX, "prosecraft-rs");
        assert_eq!(ids::MCP_SERVER_NAME, "prosecraft");
        assert_eq!(crate::craftcloud::APP_ID, ids::CRAFTCLOUD_APP_ID);
        // 0.1's sync folder inside CraftCloud's stays where 0.1 put it.
        assert_eq!(crate::craftcloud::LEGACY_SYNC_FOLDER, "ProseCraft");
    }

    #[test]
    fn baseline_is_a_full_commit_id() {
        assert_eq!(UPSTREAM_BASELINE.len(), 40);
        assert!(UPSTREAM_BASELINE.bytes().all(|b| b.is_ascii_hexdigit()));
    }
}
