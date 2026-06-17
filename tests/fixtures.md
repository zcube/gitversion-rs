# 골든 픽스쳐 목록 및 설정 커버리지

`tests/build_fixtures.sh` 가 생성하는 시나리오와, 각 설정(config) 키의 값별 분기
커버리지를 정리한다. golden 값은 .NET GitVersion 6.7.0 으로 생성하며,
`tests/fixtures.rs` 가 우리 엔진 출력과 비교한다.

- 재생성: `GITVERSION_BIN=/opt/homebrew/bin/gitversion ./tests/build_fixtures.sh`
- 현재 시나리오 수: **98**

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

### 버전 소스 / 증분 메시지
| 시나리오 | 검증 내용 |
|---|---|
| semver_minor_main / semver_major_main | +semver: minor/major |
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
| loose_four_part_tag / loose_format_four_part | 4-part 태그 (Strict 거부 / Loose 인식) |
| loose_format_partial_tag / loose_format_branch | 부분 버전 (Loose) |
| label_mismatch_prerelease | label 불일치 태그 무시 |
| cd_prerelease_numbered_tag | CD + 높은 pre-release 번호 태그 |
| branch_dot_in_name | 브랜치명 점 sanitize |

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
| increment (브랜치별 직접) | ❌ | ❌ Major/Minor/None/Inherit |
| mode (deployment) | ✅ ContinuousDelivery, Manual (mainline) | ❌ ContinuousDeployment(직접) |
| commit-message-incrementing | ✅ Disabled, MergeMessageOnly | ❌ Enabled(명시) |
| tag-prefix | ✅ 기본, "ver" | — |
| next-version | ✅ 정수/부분/full/pre-release | — |
| semantic-version-format | ✅ Strict, Loose | — |
| commit-date-format | ✅ 커스텀 | — |
| ignore | ✅ sha, commits-before, paths | — |
| assembly-versioning-scheme | ✅ Major, MajorMinor, MajorMinorPatchTag, None | ❌ MajorMinorPatch(기본 명시) |
| assembly-versioning-format | ✅ 커스텀 | — |
| assembly-file-versioning-scheme | ❌ | ❌ 전체 |
| assembly-file-versioning-format | ❌ | ❌ 전체 |
| assembly-informational-format | ❌ | ❌ 커스텀 |
| prevent-increment.of-merged-branch | ✅ true | ❌ false(명시) |
| prevent-increment.when-branch-merged | ✅ true | ❌ false(명시) |
| prevent-increment.when-current-commit-tagged | ✅ false | ❌ true(명시) |
| tracks-release-branches | ✅ true (develop) | — |
| track-merge-target | ❌ | ❌ true/false |
| track-merge-message | ✅ true(기본, merge_pr) | ❌ false |
| tag-pre-release-weight | ✅ 60000 | — |
| pre-release-weight | ❌ | ❌ 커스텀 |
| label (브랜치) | ✅ 기본(alpha 등) | ❌ 커스텀 직접 |
| label-number-pattern | ❌ | ❌ 커스텀 |
| version-in-branch-pattern | ✅ 기본 | ❌ 커스텀 |
| merge-message-formats | ✅ 내장 8종 | ❌ 커스텀 추가 |
| source-branches / is-source-branch-for | ❌ | ❌ 커스텀 |
| update-build-number | ❌ | ❌ (출력 영향 적음) |
| semantic-version-threshold | ✅ 1.0.0 | — |

### 다음 추가 대상 (우선순위)
1. mode: ContinuousDeployment (직접)
2. increment: 브랜치별 직접 적용 (Major/Minor/Inherit) — 전역 override 우회
3. assembly-file-versioning-scheme / assembly-informational-format
4. label 커스텀 / label-number-pattern
5. version-in-branch-pattern 커스텀
6. merge-message-formats 커스텀
7. track-merge-message: false
8. source-branches / is-source-branch-for
