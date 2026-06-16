//! GitVersion (Rust 포트) 진입점. 실제 로직은 lib 의 `app` 모듈에 있다(i18n t! 사용 위해).

fn main() {
    gitversion::app::main();
}
