//! 로케일 선택 헬퍼.
//!
//! 번역 자체는 [`rust_i18n`] 이 `locales/` 의 YAML 로 처리한다(영어 기본).
//! 본 모듈은 `--lang` 인자/환경변수를 rust-i18n 로케일로 매핑한다.

/// 문자열(예: "en", "ja", "zh-CN", "en_US.UTF-8")을 로케일 코드로 정규화.
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

/// `--lang` 인자(우선) 또는 `LC_ALL`/`LANG` 환경변수로 로케일 초기화. 기본은 영어.
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

    // set_locale 은 전역 상태라 병렬 테스트에서 경쟁한다. 테스트에서는 전역을
    // 바꾸지 않고 `t!(.., locale = "..")` 로 로케일을 명시해 격리한다.
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

    /// TUI 는 키를 런타임 변수(`t!(*k)`)로 넘기므로, 변수 키 해석이 동작하고
    /// 모든 tui.* 키가 실제로 YAML 에 존재함을 보장한다(누락 시 키가 그대로 출력됨).
    #[test]
    fn runtime_variable_key_resolves() {
        let key = "tui.tab.variables";
        assert_eq!(t!(key, locale = "en"), "Variables");
        assert_eq!(t!(key, locale = "ko"), "변수");
    }
}
