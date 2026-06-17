# 골든 픽스쳐 목록 및 설정 커버리지

`tests/build_fixtures.sh` 가 생성하는 시나리오와, 각 설정(config) 키의 값별 분기
커버리지를 정리한다. golden 값은 .NET GitVersion 6.7.0 으로 생성하며,
`tests/fixtures.rs` 가 우리 엔진 출력과 비교한다.

- 재생성: `GITVERSION_BIN=/opt/homebrew/bin/gitversion ./tests/build_fixtures.sh`
- 현재 시나리오 수: **145**
- golden 생성/비교 모두 **캐시·부수효과 배제**: record 는 `/nocache /nonormalize`
  (.NET 이 저장소 refs/브랜치를 수정하지 못함), 비교(fixtures.rs)는 `calculate()`
  직접 호출. 검증 결과 .NET 호출 전후 refs/출력 동일, tar 에 .NET 흔적 없음.

## 픽스쳐 시나리오

### 기본 / Main 브랜치
| 시나리오 | 검증 내용 |
|---|---|
| main_3commits | main, 태그 없이 3커밋 |
| main_tag_plus2 | v1.0.0 태그 후 2커밋 |
| main_current_tagged | HEAD 커밋에 태그 |
| main_long_no_tag | 태그 없이 12커밋 (VersionSourceDistance 기준점) |
| main_deep_after_tag | v1.0.0 후 10커밋 (distance=10) |
| multi_tag_deep | 두 태그 사이 긴 거리, 가장 가까운 태그 선택 |
| multiple_tags | 다중 태그, 최신 우선 |

### GitFlow (develop / feature / release / hotfix / support)
| 시나리오 | 검증 내용 |
|---|---|
| develop_plus1 / develop_plus2 | develop 분기 후 커밋 |
| develop_8commits | develop 8커밋 (PreReleaseNumber=8) |
| feature_off_develop | develop 위 feature |
| feature_off_main | develop 없는 feature (main Patch 상속) |
| feature_deep_develop | develop 5 후 feature 7 |
| feature_semver_major | feature + +semver:major |
| release_from_main | release 브랜치 버전 추출 |
| release_plus_commits | release 후 추가 커밋 |
| release_deep | main 4 후 release 7 |
| hotfix_branch / hotfix_deep | hotfix 브랜치 (얕음/깊음) |
| support_branch / support_deep | support 브랜치 |
| gitflow_full_cycle | develop+feature+release 완전 사이클 |
| develop_multi_feature | develop 위 다중 feature 병합 |
| multi_release_cycle | 여러 릴리스 사이클 |
| track_release_develop | develop 에서 release 추적 (TrackReleaseBranches) |

### GitHubFlow
| 시나리오 | 검증 내용 |
|---|---|
| githubflow_main / githubflow_release | 기본 / release 브랜치 |
| githubflow_deep_main / githubflow_deep_feature | 깊은 체인 |
| githubflow_feature_semver | feature + +semver:minor |
| githubflow_feature_tagged | HEAD 태그 (when-current-commit-tagged: false) |
| githubflow_release_prevent | prevent-increment of-merged-branch=true, when-branch-merged=false |
| githubflow_multi_pr | 순차 PR 병합 |

### TrunkBased
| 시나리오 | 검증 내용 |
|---|---|
| trunkbased_feature_tagged / trunkbased_hotfix_tagged / trunkbased_unknown_tagged | HEAD 태그 + when-current-commit-tagged |
| trunkbased_main_merged_feature | feature 병합 시 of_merged_branch=true |
| trunkbased_long / trunkbased_deep_feature | 깊은 체인 |

### Mainline
| 시나리오 | 검증 내용 |
|---|---|
| mainline_3commits / mainline_tag / mainline_minor | 기본 |
| mainline_merge | feature 병합 증분 consolidate |
| mainline_midtag / mainline_pretag_merge | 중간 태그 / pre-release 병합 |
| mainline_cd | ContinuousDelivery 모드 (번호=distance) |
| mainline_manual | Manual 모드 |
| mainline_long / mainline_tag_mixed / mainline_deep_feature | 깊은 체인 / 혼합 +semver |
| mainline_when_branch_merged | when-branch-merged=true 증분 미적용 |
| cfg_mainline_custom_no_source | custom 전 필드 미지정, increment None+label literal(1.0.0-{BranchName}) |

### 버전 소스 / 증분 메시지
| 시나리오 | 검증 내용 |
|---|---|
| semver_minor_main / semver_major_main | +semver: minor/major |
| semver_breaking | +semver: breaking (major 별칭) |
| semver_none | +semver: none (증분 억제) |
| cfg_custom_major_bump | 커스텀 major-bump "BREAKING" (2.0.0) |
| cfg_custom_minor_bump | 커스텀 minor-bump "^feat" (1.1.0) |
| cfg_custom_bump_overrides | 커스텀 패턴이 기본 +semver 대체 (1.0.1) |
| cfg_custom_bump_multi | 커스텀 + 복수 커밋, 혼재 시 최고 major (2.0.0) |
| cfg_custom_bump_multi_minor | 커스텀 + 복수 feat, minor 1회 (1.1.0) |
| main_long_with_minor / main_long_with_major | 긴 체인 중간 +semver |
| main_mixed_semver | 다중 +semver (최고 우선) |
| develop_with_semver | develop +semver |

### 설정(config) 키별 분기
| 시나리오 | 설정 | 값 |
|---|---|---|
| githubflow_main 등 | workflow | GitHubFlow/v1 |
| trunkbased_* | workflow | TrunkBased/preview1 |
| mainline_* | strategies | Mainline |
| nextversion_config / nextversion_partial | next-version | 2.0.0 / "1" |
| tag_beats_nextversion / nextversion_beats_tag | next-version | 0.5.0 / 3.0.0 |
| nextversion_label_mismatch | next-version | 2.0.0-beta (label 불일치) |
| tagprefix_custom / tagprefix_mixed | tag-prefix | "ver" |
| semver_loose / loose_format_* | semantic-version-format | Loose |
| semver_strict / strict_partial_tag | semantic-version-format | Strict |
| commitdate_format | commit-date-format | "yyyy.MM.dd" |
| ignore_sha / ignore_before / ignore_paths | ignore | sha / commits-before / paths |
| assembly_format | assembly-versioning-format | 커스텀 |
| assembly_scheme_major/majorminor/patch_tag/none | assembly-versioning-scheme | Major/MajorMinor/MajorMinorPatchTag/None |
| stable_weighted | tag-pre-release-weight | 60000 |
| semantic_threshold | semantic-version-threshold | "1.0.0" |
| prevent_increment_merged | prevent-increment.of-merged-branch | true |
| cfg_msg_inc_disabled | commit-message-incrementing | Disabled |
| cfg_msg_inc_mergeonly | commit-message-incrementing | MergeMessageOnly |
| cfg_increment_major | increment | Major (전역; main override 로 1.0.1) |
| cfg_increment_none | increment | None (전역; main override 로 1.0.1) |
| cfg_branch_increment_major | branches.main.increment | Major (실제 적용 2.0.0) |
| cfg_branch_increment_minor | branches.main.increment | Minor (실제 적용 1.1.0) |
| cfg_mode_cd_branch | branches.main.mode | ContinuousDeployment |
| cfg_assembly_file_scheme | assembly-file-versioning-scheme | MajorMinor |
| cfg_assembly_info_format | assembly-informational-format | 커스텀 |
| cfg_branch_label_custom | branches.feature.label | preview |
| cfg_version_pattern_custom | version-in-branch-pattern | 커스텀(separator split 검증) |
| cfg_merge_format_custom | merge-message-formats | 커스텀 |
| cfg_track_merge_msg_false | branches.main.track-merge-message | false |
| cfg_prerelease_weight | branches.feature.pre-release-weight | 5000 |
| cfg_assembly_file_format | assembly-file-versioning-format | 커스텀 |
| cfg_msg_inc_enabled | commit-message-incrementing | Enabled |
| cfg_assembly_scheme_default | assembly-versioning-scheme | MajorMinorPatch |
| cfg_pr_label_number | label-number-pattern | PR 번호 추출(pull/123/merge) |
| cfg_increment_inherit | branches.feature.increment | Inherit(부모 Major 상속) |
| cfg_branch_increment_none | branches.main.increment | None(1.0.0 유지) |
| cfg_assembly_file_scheme_none | assembly-file-versioning-scheme | None(빈 문자열) |
| cfg_label_number_custom | branches.feature.label-number-pattern | 커스텀 |
| cfg_assembly_file_scheme_major | assembly-file-versioning-scheme | Major |
| cfg_track_merge_target_false | branches.develop.track-merge-target | false |
| cfg_source_branches | branches.feature.source-branches | [main] |
| cfg_is_source_branch_for | branches.main.is-source-branch-for | [custom](Major 상속 2.0.0) |
| cfg_source_branches_inherit | branches.custom.source-branches | [main](Major 상속 2.0.0) |
| cfg_source_branches_label_inherit | source-branches label 상속 | label 미지정이 main "" 상속(2.0.0-1) |
| cfg_custom_no_source | custom 브랜치 전 필드 미지정 | increment None(증분 없음)+label literal(1.0.0-{BranchName}) |
| cfg_gitflow_unknown | unknown 브랜치(misc/foo) | GitFlow |
| cfg_githubflow_unknown | unknown 브랜치(misc/foo) | GitHubFlow |
| cfg_release_mode_cd | branches.release.mode | ContinuousDeployment |
| cfg_develop_mode_manual | branches.develop.mode | ManualDeployment |
| cfg_nextversion_loose_partial | semantic-version-format + next-version | Loose + "1"(1.0.0) |
| cfg_nextver_integer | next-version 정수 | 2(setter 가 "2.0" 보정) |
| cfg_nextver_build | next-version | "1.0.0+build5"(build metadata) |
| cfg_nextver_prerelease | next-version | "1.0.0-beta.3"(label 불일치 무시) |
| cfg_empty_tagprefix | tag-prefix | ""(빈 prefix, v태그 무시) |

### 머지 메시지 / 태그 파싱 엣지
| 시나리오 | 검증 내용 |
|---|---|
| merge_release / merge_pr | 머지 메시지 전략 |
| merge_support_ignored | release 아닌 병합 무시 |
| merge_bitbucket_v7 | BitBucketPullv7 멀티라인 포맷 |
| dual_release_tag | 동일 커밋 릴리스 태그 2개, 최고 선택 |
| prerelease_release_same_commit | release 가 pre-release 보다 우선 |
| numeric_prerelease_tag | numeric-only pre-release (v1.0.0-1) |
| prerelease_tag | pre-release 태그 후 커밋 |
| annotated_tag | 어노테이티드 태그(git tag -a) peel 인식 |
| loose_four_part_tag / loose_format_four_part | 4-part 태그 (Strict 거부 / Loose 인식) |
| loose_format_partial_tag / loose_format_branch | 부분 버전 (Loose) |
| label_mismatch_prerelease | label 불일치 태그 무시 |
| cd_prerelease_numbered_tag | CD + 높은 pre-release 번호 태그 |
| branch_dot_in_name | 브랜치명 점 sanitize |

### git 상태 엣지
| 시나리오 | 검증 내용 |
|---|---|
| detached_head_main | detached HEAD, main 유일 tip 이면 main 으로 계산 |
| detached_head_feature | detached HEAD, feature/y 유일 tip 이면 feature/y 로 계산 |
| detached_no_branch | detached HEAD + 여러 브랜치 동일 커밋, "(no branch)" label sanitize |
| branch_underscore_label | feature/a_b 의 label 은 "a-b"(SanitizeName: 비영숫자는 -) |

### 빌드 에이전트
| 시나리오 | 검증 내용 |
|---|---|
| buildagent_repo | 각 CI 어댑터 출력 golden |

---

## 설정 키 커버리지

목표: **모든 설정 키의 모든 분기 값을 골든 테스트로 커버**. ✅=커버, ❌=미커버.

| 설정 키 | 테스트된 값 | 미테스트 값 |
|---|---|---|
| workflow | ✅ GitFlow(기본), GitHubFlow/v1, TrunkBased/preview1 | — |
| strategies | ✅ Mainline, ConfiguredNextVersion 등 | ❌ 개별 조합 일부 |
| increment (전역) | ✅ Major, None, Patch(기본) | ❌ Minor(직접), Inherit |
| increment (브랜치별 직접) | ✅ Major, Minor, None, Inherit | — |
| mode (deployment) | ✅ ContinuousDelivery, ContinuousDeployment, ManualDeployment (전역·브랜치별) | — |
| 워크플로 × unknown 브랜치 | ✅ GitFlow, GitHubFlow | ❌ TrunkBased unknown(별도 있음) |
| commit-message-incrementing | ✅ Enabled, Disabled, MergeMessageOnly | — |
| *-version-bump-message | ✅ 기본(+semver), 커스텀 패턴(단일·복수 커밋), 기본 대체 (잘못된 정규식은 에러) | — |
| tag-prefix | ✅ 기본, "ver", 빈값 (잘못된 정규식은 에러) | — |
| next-version | ✅ full/pre-release/build-metadata(Strict), 부분/정수(Loose) | Strict+부분버전은 계산 에러(원본 동작) |
| next-version 정수 보정 | ✅ "1"은 "1.0", "2"는 "2.0"(원본 setter) | — |
| semantic-version-format | ✅ Strict, Loose | — |
| commit-date-format | ✅ 커스텀 | — |
| ignore | ✅ sha, commits-before, paths | — |
| assembly-versioning-scheme | ✅ Major, MajorMinor, MajorMinorPatch, MajorMinorPatchTag, None | — |
| assembly-versioning-format | ✅ 커스텀, 알 수 없는 토큰은 에러(원본 동작) | — |
| assembly-file-versioning-scheme | ✅ Major, MajorMinor, None | ❌ MajorMinorPatch 등 |
| assembly-file-versioning-format | ✅ 커스텀 | — |
| assembly-informational-format | ✅ 커스텀 | — |
| prevent-increment.of-merged-branch | ✅ true | ❌ false(명시) |
| prevent-increment.when-branch-merged | ✅ true | ❌ false(명시) |
| prevent-increment.when-current-commit-tagged | ✅ false | ❌ true(명시) |
| tracks-release-branches | ✅ true (develop) | — |
| track-merge-target | ✅ false | ❌ true(명시) |
| track-merge-message | ✅ true(기본), false | — |
| tag-pre-release-weight | ✅ 60000 | — |
| pre-release-weight | ✅ 5000(커스텀) | — |
| label (브랜치) | ✅ 기본(alpha 등), 커스텀(preview), sanitize(비영숫자는 -) | — |
| label-number-pattern | ✅ 기본(PR 번호), 커스텀 | — |
| version-in-branch-pattern | ✅ 기본, 커스텀(separator split) | — |
| merge-message-formats | ✅ 내장 8종, 커스텀 | — |
| source-branches | ✅ [main], increment Inherit 상속 | — |
| is-source-branch-for | ✅ [custom], increment Inherit 상속 | — |
| update-build-number | ❌ | ❌ (출력 영향 적음) |
| semantic-version-threshold | ✅ 1.0.0 | — |

### 남은 갭 (출력 영향 적거나 변별 어려움)
1. track-merge-target: true (기본값 명시)
2. assembly-file-versioning-scheme: MajorMinorPatch / MajorMinorPatchTag
3. update-build-number (CI 빌드넘버 갱신 여부 — 버전 출력에 영향 없음)

### custom 브랜치 / 상속 동작 (원본 로직 일치)
- **label 토큰 치환**(resolve_label): named capture 만 placeholder(각 값 SanitizeName),
  capture 없으면 토큰 literal 유지(예: `^custom/` + `{BranchName}` 은 그대로). 세그먼트
  fallback·최종 전체 sanitize 없음. 원본 BuildLabelPlaceholders + FormatWith.
- **label/increment source 상속**: 원본 `BranchConfiguration.Inherit` 처럼 label 미지정 시
  source-branches 부모에서 상속(inherit_label), increment Inherit 도 source 상속
  (resolve_increment). 예: custom + source:[main] 은 main 의 label("")·increment(Major)
  상속(2.0.0-1).
- **increment 를 끝까지 못 풀면 None**(증분 없음): 원본 ToVersionField 는 Inherit 를
  변환하지 못하므로 None 으로 귀결한다. **임의 Patch fallback 을 쓰지 않는다**
  (resolve_increment, resolve_inherit_via_git 둘 다). 예: custom + source 없음은
  1.0.0-{BranchName}(증분 없음). cfg_custom_no_source 로 검증.
- 잔여: mode/track 등 그 외 미지정 필드의 source 상속은 미구현(드문 엣지, 프리셋엔 무관).
- **mainline fallback 검토 완료**: mainline 도 임의 Patch/0.0.0 fallback 없음.
  trunk_default 는 strategy_to_field(resolve_increment 결과)라 Inherit 미해결 시 None.
  merge_branch_increment 는 병합 브랜치의 명시 increment 만 floor 로 적용(Inherit 면 None,
  원본과 일치). mainline+custom 전 필드 미지정도 1.0.0-{BranchName} 로 원본 일치
  (cfg_mainline_custom_no_source).

핵심 설정 키의 주요 값 분기는 모두 골든 테스트로 커버됨.
