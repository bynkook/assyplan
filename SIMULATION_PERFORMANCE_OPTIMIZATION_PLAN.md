# Simulation Performance Optimization Plan

이 문서는 현재까지 구현된 1순위 최적화(floor prefilter)와, 아직 구현하지 않은 2·3·4순위 계산 성능 효율화 방안을 함께 정리한 계획 문서다.

## 배경

Simulation Mode에서 `lower_floor_completion_ratio_threshold`가 `1.0`보다 작아지면 동시에 열리는 floor 수가 증가한다.
이때 실제 병목은 조건문 비교 자체가 아니라, 열린 floor가 많아지면서 아래 반복이 증폭되는 데 있다.

1. workfront별 candidate 수집 반복
2. seed별 complete plan 평가 반복
3. `try_build_pattern()` 기반 패턴 탐색 반복
4. `check_bundle_stability()` 호출 반복

## 1순위: Floor prefilter before candidate collection

### 목표

허용되지 않은 floor의 element를 candidate 수집 초기에 제외하여, 낮은 threshold에서 불필요하게 커지는 탐색 공간을 가장 안전하게 줄인다.

### 당시 문제

기존 구조에서는 workfront별 후보를 먼저 넓게 모은 뒤, 그 다음 단계에서 floor eligibility를 다시 확인하는 흐름이 남아 있었다.
이 때문에 `lower_floor_completion_ratio_threshold`가 낮아져 eligible floor가 많아지면, 실제로 선택될 수 없는 floor의 element까지 candidate 계산 비용을 먼저 지불하게 됐다.

### 구현 내용

현재 구현은 `sim_engine.rs`에서 workfront round마다 `allowed_floors`를 먼저 계산하고, 그 집합을 candidate 수집 함수에 직접 전달한다.

적용된 핵심 변경:

- `run_scenario_internal()`에서 workfront별 `allowed_floors: HashSet<i32>` 선계산
- `committed_floor`가 있으면 singleton floor만 허용
- lock이 없으면 `is_floor_eligible_for_new_work()`를 통과한 floor만 허용
- `collect_single_candidates()`에 `allowed_floors` 인자 추가
- `collect_single_candidates_legacy()`와 `collect_single_candidates_optimized()` 양쪽 모두 floor early-skip 적용
- 후단 valid seed filtering에서도 동일 집합을 재사용하여 중복 eligibility 계산 축소

### 기대 효과

- 허용되지 않은 floor에 대한 거리 계산, connectivity 계산, 구조 가능성 판단을 조기에 차단
- threshold가 `1.0`보다 작아질수록 증가하던 candidate 수집 비용을 1차적으로 완화
- scheduling semantics 변경 없이 탐색 공간만 줄이는 low-risk 최적화 달성

### 실제 한계

이 최적화는 candidate 수집 앞단만 줄인다.
따라서 아래 비용은 여전히 남는다.

- floor별 상태 재계산
- 많은 seed에 대한 `try_build_pattern()` 호출
- `check_bundle_stability()` 중심의 비싼 안정성 검증

즉, 1순위는 가장 안전한 병목 절감이지만, low-threshold 구간의 전체 runtime 증가를 근본적으로 끝내지는 못한다.

### 검증 상태

현재 구현 후 아래 성격의 검증을 이미 통과한 상태다.

- legacy/optimized candidate collection parity 테스트
- floor 관련 제약 테스트
- upper-floor 제약 테스트
- simulation completion regression 테스트

남은 우선순위는 아래와 같다.

## 2순위: Per-round floor state cache

### 목표

같은 round 안에서 floor별 제약 상태를 element마다 반복 계산하지 않도록 줄인다.

### 현재 문제

현재는 workfront별 candidate filtering 과정에서 각 element의 floor에 대해 반복적으로 다음 판단을 간접 수행한다.

- lower floor completion ratio 충족 여부
- forced completion 구간 여부
- upper/lower installed column ratio deficit 상태

이 정보는 round 시작 시점의 `committed_floor_counts`와 `constraints`가 고정되면 floor 단위로 한 번만 계산해도 된다.

### 제안 구조

`sim_engine.rs`에 floor 상태 캐시 구조를 추가한다.

예시:

```rust
struct FloorRoundState {
    floor: i32,
    eligible_for_new_work: bool,
    forced_completion_only: bool,
    installed_columns: usize,
    total_columns: usize,
    target_upper_columns: f64,
    ratio_deficit: f64,
}
```

round 시작 시 `HashMap<i32, FloorRoundState>`를 한 번 만들고, 이후에는:

- `is_floor_eligible_for_new_work()` 대체 또는 내부 캐시 조회
- `choose_target_floor()`에서 deficit 재계산 제거
- candidate filtering 시 floor lookup만 수행

### 기대 효과

- floor 기반 조건문의 중복 계산 제거
- threshold가 낮아져 eligible floor가 늘어날 때 증가하는 반복 비용 완화
- scheduler 의도는 유지하면서도 비교적 안전한 최적화 가능

### 리스크

- round 중간에 `selected_this_sequence`가 누적되므로, 캐시 기준 시점이 `committed_ids`인지 `wf_committed_ids`인지 명확히 분리해야 한다
- 캐시가 너무 이른 시점 값이면 선택 정확도가 낮아질 수 있다

### 권장 구현 범위

우선 `committed_floor_counts` 기준 캐시만 도입하고, `selected_this_sequence`까지 반영하는 미세 최적화는 2차 작업으로 분리한다.

## 3순위: `try_build_pattern()` 호출 상한 도입

### 목표

seed 후보가 많아질 때 pattern 탐색이 선형 이상으로 불어나는 문제를 완화한다.

### 현재 문제

`floor_seeds`가 많아지면 각 seed마다 `try_build_pattern()`이 호출될 수 있다.
이 함수 내부는 pattern choice 생성과 stability 검증을 동반하므로, threshold가 낮을수록 호출 수 증가가 계산시간을 크게 끌어올린다.

### 제안 구조

seed를 전부 깊게 보지 않고, 우선순위가 높은 일부 seed만 `try_build_pattern()` 대상으로 제한한다.

예시 규칙:

1. `SingleCandidate::score(w1, w2)` 기준 정렬
2. 상위 `N`개만 `try_build_pattern()` 호출
3. 나머지는 complete plan 직접 생성만 시도하거나 skip

`N`은 고정값 또는 grid 규모 기반 동적값으로 둘 수 있다.

예시:

```rust
let seed_budget = if floor_seeds.len() > 12 { 6 } else { floor_seeds.len() };
```

### 기대 효과

- pattern 탐색 비용 상한 설정
- low-threshold 환경에서 seed 폭증 시 runtime 급증 완화
- floor interleaving 유지하면서도 탐색 폭을 제어 가능

### 리스크

- 탐색 폭이 줄어들어 일부 scenario에서 최적 step 조합을 놓칠 수 있음
- completion 품질이 seed budget에 민감할 수 있음

### 권장 구현 범위

처음에는 hard cap이 아니라 `top-K + fallback 1개` 방식으로 시작한다.

- 상위 K개 seed에 대해 full pattern 탐색
- 실패 시 나머지 seed 중 최고점 1개만 추가 탐색

이렇게 하면 성능 절감과 completion 안정성 사이 균형을 잡기 쉽다.

## 4순위: `check_bundle_stability()` 이전 cheap precheck 강화

### 목표

비싼 stability 검사를 가능한 늦게 호출하고, 값싼 구조 조건으로 먼저 탈락시킨다.

### 현재 문제

현재는 complete plan 또는 pattern choice를 만들 때 `check_bundle_stability()` 호출 비중이 높다.
threshold가 낮아지면 seed와 plan 조합이 늘어나므로 이 호출 수 역시 증가한다.

### 제안 구조

`check_bundle_stability()` 앞에 값싼 사전 필터를 추가한다.

후보 필터 예시:

1. support node 공유 수가 0이면 skip
2. girder-only closure 후보인데 perpendicular 가능성이 없으면 skip
3. column + girder 조합에서 girder가 seed upper node와 직접 연결되지 않으면 skip
4. 이미 local footprint와 지나치게 멀고, cleanup 예외 조건도 아니면 skip
5. pattern signature 상 즉시 invalid가 확정되면 skip

### 기대 효과

- expensive stability evaluation 횟수 감소
- `try_build_pattern()` 내부와 complete plan 생성부 모두에서 공통 이득 가능

### 리스크

- cheap precheck가 실제 stability 조건보다 강해지면 정당한 후보를 잘못 버릴 수 있음
- 따라서 false negative가 없어야 하며, conservative한 필터만 허용해야 함

### 권장 구현 범위

다음 세 가지만 먼저 허용한다.

- support node 0개 후보 제거
- 직접 연결 불가능한 girder 조합 제거
- 명백한 invalid signature 조기 제거

geometry 품질 기반 필터는 2차 검증이 끝난 뒤에 넣는다.

## 구현 순서 제안

1. 2순위 floor state cache
2. 3순위 `try_build_pattern()` seed budget
3. 4순위 cheap precheck 강화

이 순서가 안전한 이유:

- 2순위는 의미 변화가 거의 없는 구조 최적화
- 3순위는 탐색 폭을 줄이는 정책 변경이므로 2순위 이후 성능 차이를 보기 좋음
- 4순위는 false negative 위험이 있어 가장 나중에 넣는 편이 안전함

## 검증 계획

각 단계마다 아래를 반복한다.

1. `cargo test --release test_ab_collect_single_candidates_ -- --nocapture`
2. `cargo test --release floor_ -- --nocapture`
3. `cargo test --release upper_floor_ -- --nocapture`
4. `cargo test --release test_simulation_completes_ -- --nocapture`
5. 대표 대규모 grid에서 threshold `1.0`, `0.8`, `0.6` 실행 시간 비교

시간 비교는 동일 seed와 동일 workfront 구성으로 측정한다.

## 성공 기준

다음 조건을 동시에 만족해야 한다.

1. `threshold < 1.0`에서 runtime 증가폭이 완만해질 것
2. completion regression이 없을 것
3. floor interleaving 동작이 유지될 것
4. multi-workfront late girder cleanup regression이 재발하지 않을 것
