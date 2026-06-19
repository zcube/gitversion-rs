//! Entry point for the gitversion-rs binary. Actual logic lives in lib's `app` module (required for the i18n `t!` macro).

fn main() {
    gitversion_rs::app::main();
}
