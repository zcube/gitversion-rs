//! GitVersion 설정: 데이터 모델, 워크플로 기본값, effective 설정, 로더.

pub mod defaults;
pub mod effective;
pub mod loader;
pub mod model;

pub use model::*;
