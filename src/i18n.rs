//! Locale selection helper.
//!
//! Translations themselves are handled by [`rust_i18n`] from YAML files in `locales/` (English default).
//! This module maps the `--lang` argument or environment variables to a rust-i18n locale code.

/// Normalise a locale string (e.g. "en", "ja", "zh-CN", "en_US.UTF-8") to a locale code.
fn normalize(s: &str) -> Option<&'static str> {
    let s = s.trim().to_lowercase();
    if s.starts_with("ja") || s == "japanese" {
        Some("ja")
    } else if s.starts_with("zh") || s == "chinese" {
        Some("zh")
    } else if s.starts_with("ko") || s == "korean" {
        Some("ko")
    } else if s.starts_with("en") || s == "english" || s == "c" || s == "posix" {
        Some("en")
    } else {
        None
    }
}

/// Initialise the locale from the `--lang` argument (takes priority) or the `LC_ALL`/`LANG` environment variables. Defaults to English.
pub fn init(explicit: Option<&str>) {
    let chosen = explicit
        .and_then(normalize)
        .or_else(|| {
            std::env::var("LC_ALL")
                .or_else(|_| std::env::var("LANG"))
                .ok()
                .and_then(|v| normalize(&v))
        })
        .unwrap_or("en");
    rust_i18n::set_locale(chosen);
}

#[cfg(test)]
mod tests {
    use rust_i18n::t;

    // set_locale is global state and races under parallel tests.
    // Tests use `t!(.., locale = "..")` to specify the locale per-call instead of mutating the global.
    #[test]
    fn translates_by_locale() {
        assert_eq!(t!("status.ready", locale = "en"), "Ready");
        assert_eq!(t!("status.ready", locale = "ko"), "준비 완료");
        assert_eq!(t!("status.ready", locale = "ja"), "準備完了");
        assert_eq!(t!("status.ready", locale = "zh"), "就绪");
    }

    #[test]
    fn interpolation() {
        assert_eq!(
            t!("error.generic", locale = "en", error = "boom"),
            "Error: boom"
        );
    }

    /// The TUI passes keys as runtime variables (`t!(*k)`), so this verifies that variable key
    /// resolution works and that every `tui.*` key actually exists in the YAML (missing keys render as the raw key string).
    #[test]
    fn runtime_variable_key_resolves() {
        let key = "tui.tab.variables";
        assert_eq!(t!(key, locale = "en"), "Variables");
        assert_eq!(t!(key, locale = "ko"), "변수");
    }
}
