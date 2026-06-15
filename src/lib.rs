//! GitVersion (Rust 포트) 라이브러리 표면.
//!
//! 바이너리(`main.rs`)와 통합 테스트(`tests/`)가 공유하는 모듈을 노출한다.

// 원본 GitVersion 의 공개 API 를 옮기는 과정에서 현 바이너리가 직접 호출하지
// 않는 헬퍼(예: tags_on_commit, format_short)도 의도적으로 포함한다.
#![allow(dead_code)]

pub mod buildagent;
pub mod cache;
pub mod cli;
pub mod config;
pub mod exec;
pub mod git;
pub mod output;
pub mod remote;
pub mod tui;
pub mod version;
