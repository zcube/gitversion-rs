//! 빌드에이전트 출력 차등 테스트.
//!
//! `testdata/fixtures.tar.gz` 의 `buildagent_repo` 시나리오에는 실제 GitVersion 6.x
//! 가 각 CI 에이전트로 출력한 golden(`agent_<Name>.txt`)이 들어 있다. 이 테스트는
//! 동일 저장소에 대해 우리 엔진이 만든 변수로 `write_integration` 을 실행하고, 같은
//! 방식으로 필터링한 결과를 golden 과 비교한다.

use std::fs;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use gitversion_rs::{buildagent, config, git, version};

fn extract_fixtures() -> PathBuf {
    let archive = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/fixtures.tar.gz");
    assert!(
        archive.exists(),
        "fixture 압축이 없습니다: {}",
        archive.display()
    );
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dest = std::env::temp_dir().join(format!("gitversion-ba-{}-{}", std::process::id(), nanos));
    fs::create_dir_all(&dest).unwrap();
    let file = fs::File::open(&archive).unwrap();
    tar::Archive::new(GzDecoder::new(file))
        .unpack(&dest)
        .unwrap();
    dest
}

fn keep(line: &str) -> bool {
    // UncommittedChanges 는 작업트리의 untracked/수정 파일에 의존하는 비결정적
    // 값이라 비교에서 제외한다. golden 은 생성 시 로그/빈줄을 이미 제거했고, 우리
    // write_integration 도 명령 라인만 만들므로 그 외에는 전체 라인을 비교한다.
    // 키 대문자화(GITVERSION_UNCOMMITTEDCHANGES) 어댑터도 있으므로 대소문자 무관 비교.
    !line.is_empty() && !line.to_uppercase().contains("UNCOMMITTEDCHANGES")
}

#[test]
fn build_agents_match_real_gitversion() {
    let root = extract_fixtures();
    let agents = [
        "TeamCity",
        "AzurePipelines",
        "ContinuaCi",
        "MyGet",
        "Drone",
        "BitBucketPipelines",
        "Jenkins",
        "CodeBuild",
        "BuildKite",
        "SpaceAutomation",
        "EnvRun",
        "TravisCi",
    ];
    let mut failures = Vec::new();
    let mut checked = 0;

    // buildagent_repo: update-build-number 기본(true) / buildagent_no_ubn: false.
    // 각 시나리오의 config 에서 update-build-number 를 읽어 write_integration 에 반영해,
    // 빌드넘버 갱신 명령의 포함/제외가 설정대로 동작하는지 golden 과 비교한다.
    for scenario in ["buildagent_repo", "buildagent_no_ubn"] {
        let repo_dir = root.join(scenario);
        if !repo_dir.join("expected.json").exists() {
            failures.push(format!("{scenario} 시나리오가 없습니다"));
            continue;
        }

        let repo = git::GitRepo::discover(&repo_dir).unwrap();
        let workdir = repo
            .workdir()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| repo_dir.clone());
        let configuration = config::loader::load(None, &workdir, Some(&workdir)).unwrap();
        // update-build-number 설정 반영(미지정이면 원본 기본값 true).
        let update_build_number = configuration.update_build_number.unwrap_or(true);
        let vars = version::calculation::calculate(&repo, &configuration, None).unwrap();

        for agent_name in agents {
            let golden_path = repo_dir.join(format!("agent_{agent_name}.txt"));
            let Ok(golden) = fs::read_to_string(&golden_path) else {
                failures.push(format!(
                    "[{scenario}/{agent_name}] golden 파일 없음: {}",
                    golden_path.display()
                ));
                continue;
            };
            let agent = buildagent::by_name(agent_name).expect("알 수 없는 에이전트");
            let golden_lines: Vec<&str> = golden.lines().filter(|l| keep(l)).collect();
            let mine: Vec<String> = agent
                .write_integration(&vars, update_build_number)
                .into_iter()
                .filter(|l| keep(l))
                .collect();

            if mine.len() != golden_lines.len() {
                failures.push(format!(
                    "[{scenario}/{agent_name}] 라인 수 불일치: real={} mine={}",
                    golden_lines.len(),
                    mine.len()
                ));
                continue;
            }
            for (i, (g, m)) in golden_lines.iter().zip(mine.iter()).enumerate() {
                if g != m {
                    failures.push(format!(
                        "[{scenario}/{agent_name}] line {i}: real={g:?} mine={m:?}"
                    ));
                }
            }
            checked += 1;
        }
    }

    let _ = fs::remove_dir_all(&root);

    assert!(
        failures.is_empty(),
        "{}개 에이전트 검증 중 불일치:\n{}",
        checked,
        failures.join("\n")
    );
    println!("{checked}개 빌드에이전트 출력이 실제 GitVersion 과 일치");
}
