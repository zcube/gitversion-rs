//! Fixture 기반 차등(differential) 통합 테스트.
//!
//! `testdata/fixtures.tar.gz` 에는 시나리오별 git 저장소와, 실제 GitVersion 6.x
//! 바이너리가 생성한 golden 기대값(`expected.json`)이 들어 있다. 이 테스트는
//! 압축을 임시 디렉터리로 풀어 우리 엔진의 출력을 golden 값과 비교한다.
//! 따라서 테스트 시점에는 git/gitversion 바이너리가 필요 없으며 재현 가능하다.
//!
//! fixture 재생성:  `./tests/build_fixtures.sh`

use std::fs;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use gitversion_rs::{config, git, version};
use serde_json::Value;

/// 비교할 출력 변수 키(버전 핵심 필드). Sha/CommitDate/Weighted 등은 제외.
const COMPARED_KEYS: &[&str] = &[
    "FullSemVer",
    "SemVer",
    "MajorMinorPatch",
    "Major",
    "Minor",
    "Patch",
    "PreReleaseLabel",
    "PreReleaseLabelWithDash",
    "PreReleaseNumber",
    "PreReleaseTag",
    "PreReleaseTagWithDash",
    "BranchName",
    "EscapedBranchName",
    "CommitDate",
    "AssemblySemVer",
    "AssemblySemFileVer",
    "InformationalVersion",
    "WeightedPreReleaseNumber",
    "VersionSourceDistance",
    "VersionSourceIncrement",
    "Sha",
    "ShortSha",
    // 주의: UncommittedChanges 는 작업트리의 untracked/수정 파일에 의존하는
    // 비결정적 값이라 고정 fixture 로 단언하지 않는다(구현은 별도 검증됨).
];

/// 압축된 fixture 를 유니크한 임시 디렉터리로 푼다.
fn extract_fixtures() -> PathBuf {
    let archive = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/fixtures.tar.gz");
    assert!(
        archive.exists(),
        "fixture 압축이 없습니다: {} (먼저 ./tests/build_fixtures.sh 실행)",
        archive.display()
    );

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dest = std::env::temp_dir().join(format!(
        "gitversion-fixtures-{}-{}",
        std::process::id(),
        nanos
    ));
    fs::create_dir_all(&dest).unwrap();

    let file = fs::File::open(&archive).unwrap();
    let mut tar = tar::Archive::new(GzDecoder::new(file));
    tar.unpack(&dest).unwrap();
    dest
}

/// 실제 GitVersion JSON 값을 우리 to_map 과 비교 가능한 문자열로 정규화.
fn normalize(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

#[test]
fn fixtures_match_real_gitversion() {
    let root = extract_fixtures();

    // tar 가 './<name>/' 구조로 풀리므로 한 단계 들어간다.
    let mut scenario_dirs: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(&root).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() && p.join("expected.json").exists() {
            scenario_dirs.push(p);
        }
    }
    scenario_dirs.sort();
    assert!(
        !scenario_dirs.is_empty(),
        "시나리오를 찾지 못했습니다: {}",
        root.display()
    );

    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for dir in &scenario_dirs {
        let name = dir.file_name().unwrap().to_string_lossy().to_string();

        // golden 값 로드.
        let expected_text = fs::read_to_string(dir.join("expected.json")).unwrap();
        let expected: Value = serde_json::from_str(&expected_text).unwrap();

        // 우리 엔진 실행.
        let repo = match git::GitRepo::discover(dir) {
            Ok(r) => r,
            Err(e) => {
                failures.push(format!("[{name}] 저장소 오픈 실패: {e}"));
                continue;
            }
        };
        let workdir = repo
            .workdir()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| dir.clone());
        let configuration = config::loader::load(None, &workdir, Some(&workdir)).unwrap();
        let vars = match version::calculation::calculate(&repo, &configuration, None) {
            Ok(v) => v,
            Err(e) => {
                failures.push(format!("[{name}] 계산 실패: {e}"));
                continue;
            }
        };
        let actual = vars.to_map();

        for key in COMPARED_KEYS {
            let exp = normalize(expected.get(*key));
            let got = actual.get(*key).cloned().unwrap_or_default();
            if exp != got {
                failures.push(format!(
                    "[{name}] {key}: 기대(real)={exp:?} 실제(mine)={got:?}"
                ));
            }
        }
        checked += 1;
    }

    // 정리(임시 디렉터리 제거).
    let _ = fs::remove_dir_all(&root);

    if !failures.is_empty() {
        panic!(
            "{}개 시나리오 중 불일치 {}건:\n{}",
            checked,
            failures.len(),
            failures.join("\n")
        );
    }
    println!("{checked}개 시나리오 모두 실제 GitVersion 과 일치");
}
