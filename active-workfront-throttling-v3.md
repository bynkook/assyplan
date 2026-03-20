# Active Workfront Throttling v3

이 문서는 파일명은 유지하지만, 내용은 현재 구현되어 있는 코드 기준의 active workfront 제어 방식만 다룬다.

과거 v1/v2 시기의 same-floor throttling 상태기계, floor rebase, spatial rebase, forced completion 기반 제어는 현재 canonical 구현이 아니다. 따라서 이 문서는 예전 실험 로그가 아니라 현재 sim_engine.rs 에서 실제로 살아 있는 흐름을 기준으로 읽어야 한다.

## 1. 현재 한 줄 요약

현재 코드는 별도의 throttling 상태기계를 사용하지 않는다.

- 각 global step cycle 에서 eligible workfront 집합을 구한다.
- 남은 미설치 부재 수에 비례해 이번 round 에 active 로 돌릴 workfront 수를 계산한다.
- eligible 집합에서 round-robin 으로 active set 을 고른다.
- 각 active workfront 는 round 당 최대 1개 부재만 선택한다.
- local buffer 가 complete pattern + stability pass 가 될 때만 LocalStep 을 만들고, cycle 종료 시 global SimStep 으로 병합한다.

즉, 현재 구현은 same-floor 정체를 감지해서 특정 workfront 몇 개만 남기는 throttling 보다는, remaining work 기반 active-set 축소와 retry loop 기반 local-step 형성에 가깝다.

## 2. 현재 active workfront 제어의 목적

현재 active workfront 제어의 목적은 다음 두 가지다.

- large model 후반부로 갈수록 모든 workfront 를 계속 동시에 돌리지 않고, 남은 작업량에 맞게 active 수를 줄여 충돌과 공회전을 줄인다.
- 그렇더라도 Sequence != Step 원칙은 유지하고, 각 workfront 의 local buffer 기반 패턴 완성 흐름은 깨지지 않게 한다.

현재 코드는 대표 workfront 선별이나 same-floor sparse contention 전용 상태기계 대신, 단순하고 구조적인 round 제어를 택한다.

## 3. 현재 구현에서 active workfront 는 어떻게 정해지는가

### 3.1 eligible workfront

한 cycle 의 각 round 시작 시 eligible workfront 는 아래 조건으로 정해진다.

- 아직 그 cycle 안에서 LocalStep 생성에 성공하지 않은 workfront

즉, 이미 같은 cycle 에서 local step 을 만든 workfront 는 해당 cycle 의 남은 round 에서 제외된다.

코드상 기준:

```rust
let eligible_wfs: Vec<&SimWorkfront> = workfronts
    .iter()
    .filter(|wf| !cycle_completed_wf.contains(&wf.id))
    .collect();
```

### 3.2 active count 계산

eligible workfront 전체를 매 round 전부 돌리지 않는다. 현재 round 에 실제로 active 로 사용할 workfront 수는 남은 미설치 부재 수를 기준으로 계산한다.

핵심 계산식:

```rust
let current_remaining = total_elements.saturating_sub(committed_ids.len());
let active_count = if current_remaining == 0 {
    1
} else {
    let proportional = std::cmp::max(1, current_remaining * workfronts.len() / total_elements);
    let elements_per_wf = (total_elements + workfronts.len() - 1) / workfronts.len();
    let retirement_cap = std::cmp::max(1, (current_remaining + elements_per_wf - 1) / elements_per_wf);
    std::cmp::min(proportional, retirement_cap)
};
```

의미:

- 초반부에는 남은 작업량이 많아서 active workfront 수가 크게 유지된다.
- 후반부에는 남은 작업량이 줄어들수록 active workfront 수도 자연스럽게 줄어든다.
- 별도 CapTwo, CapOne, Cooldown 같은 throttling 모드 전환은 없다.

### 3.3 active workfront 선택 방식

active 집합은 eligible 집합에서 round-robin 으로 선택한다.

핵심 계산식:

```rust
let start_index = (cycle_round_index - 1) % eligible_wfs.len();
let active_wfs: Vec<&SimWorkfront> = eligible_wfs
    .iter()
    .cycle()
    .skip(start_index)
    .take(active_count.min(eligible_wfs.len()))
    .copied()
    .collect();
```

의미:

- 특정 workfront 만 계속 우선권을 독점하지 않게 한다.
- same-floor 대표 선출 heuristic 대신, 라운드별 회전과 active-count 축소로 충돌을 완화한다.

## 4. 현재 구현에서 더 이상 없는 것

현재 canonical 구현에는 아래 상태와 정책이 없다.

- FloorThrottleState
- FloorThrottleMode::Observe, CapTwo, CapOne, Cooldown
- WorkfrontActivationInfo
- FloorCongestionInfo
- zone representative 선별 로직
- planned_pattern
- last_failed_floor
- runtime_anchor_x, runtime_anchor_y
- rebase_cooldown_rounds
- floor_rebase_count, spatial_rebase_count
- lower_floor_forced_completion_threshold

즉, 예전 문서에서 설명하던 상위 2개만 유지, single-WF finish mode, floor rebase, spatial rebase는 현재 코드 truth 가 아니다.

## 5. 현재 round 내부의 실제 흐름

### 5.1 bootstrap 이전

아직 안정 구조가 전혀 없으면 bootstrap 경로를 사용한다.

- 조건: stable_ids.is_empty() && cycle_local_steps.is_empty()
- workfront anchor 근처 bootstrap bundle 후보를 만든다.
- w1/w2/w3 점수로 weighted sampling 한다.
- 선택된 bootstrap pattern 전체를 그 workfront buffer 에 넣는다.

즉, 초기 단계는 active throttling 이 아니라 bootstrap bundle 선택이 핵심이다.

### 5.2 bootstrap 이후

bootstrap 이후에는 각 active workfront 가 증분 후보를 하나씩 시도한다.

- 각 active workfront 는 round 당 최대 1개 부재만 선택한다.
- 현재 buffer 와 stable context 를 기준으로 allowed_floors 를 계산한다.
- committed_floor 가 있으면 그 층만 허용한다.
- 각 후보는 locality, structural possibility, floor gate, buffer mask 를 통과해야 한다.
- 그 뒤 retry loop 로 현재 local step 을 실제로 진전시키는 후보를 찾는다.

## 6. retry loop 가 현재 구현의 핵심이다

현재 active workfront 제어를 이해할 때 가장 중요한 것은 throttling 이 아니라 single-candidate retry loop 다.

구조는 아래와 같다.

1. candidate pool 을 만든다.
2. 점수 기반 weighted random choice 로 후보 1개를 뽑는다.
3. 그 후보가 현재 buffer 를 실제로 진전시키는지 검사한다.
4. invalid pattern 또는 unstable complete 면 버린다.
5. 실패한 후보를 제외한 나머지로 다시 시도한다.
6. 통과한 후보가 있으면 그 1개만 이번 round 설치 후보가 된다.

즉, 현재 구현은 여러 후보를 한 번에 밀어 넣고 나중에 유효 subset 만 고르는 방식이 아니다.

## 7. floor 제약은 단순화되었다

현재 uncommitted workfront 의 floor 허용 조건은 아래 두 제약만 사용한다.

- upper_floor_column_rate_threshold
- lower_floor_completion_ratio_threshold

현재 코드에서 더 이상 없는 것:

- forced completion threshold 기반 하층 마감 우선
- threshold 이하일 때 relaxed locality search 허용
- floor 말단 구간 step emission 지연

즉, active workfront 제어와 floor 제약은 현재 아래처럼 단순화되어 있다.

- floor gate 는 allowed_floors 계산으로 먼저 적용한다.
- active workfront 제어는 remaining-work 기반 active_count 축소로 처리한다.

## 8. rollback 도 단순화되었다

현재 rollback 은 buffer-only rollback 이다.

발생 조건:

- buffer 가 Invalid 가 되었을 때
- complete pattern 이 되었지만 stability fail 로 Infeasible 가 되었을 때
- committed_floor 상태에서 더 이상 유효 후보가 없을 때

처리 방식:

- 현재 buffer 부재만 owned_ids 에서 제거
- buffer_sequences 비움
- committed_floor 해제

현재는 rollback 이후에 별도 rebase 상태를 심지 않는다.

## 9. metrics 에 남아 있는 throttle/rebase 필드는 placeholder 다

ScenarioMetrics 에는 아직 아래 필드가 남아 있다.

- throttle_events
- floor_rebase_events
- spatial_rebase_events

하지만 현재 sim_engine.rs 에서는 이 값들을 모두 0 으로 채운다.

즉, 이 필드들은 현재 활성 기능을 뜻하지 않는다. 과거 telemetry 표면이 남아 있는 것이며, 현재 build 에서 active-workfront-throttling-v3 상태기계를 의미하지 않는다.

## 10. 현재 문서 기준 code truth

현재 build 기준 code truth 는 아래와 같다.

- 별도 throttling 상태기계는 없다.
- active workfront 수는 remaining work 기반으로 비례 축소된다.
- active set 은 eligible workfront 에서 round-robin 으로 선택된다.
- 각 active workfront 는 round 당 최대 1개 부재만 시도한다.
- local step 형성은 single-candidate retry loop + buffer classification + complete-pattern emission 으로 제어된다.
- rollback 은 buffer-only rollback 이다.
- throttle/rebase metrics 필드는 현재 0 placeholder 다.

## 11. 후속 수정 시 주의사항

이 문서를 기준으로 후속 작업할 때 지켜야 할 점은 다음과 같다.

- 예전 CapTwo, CapOne, Cooldown 상태기계를 전제로 수정하지 않는다.
- lower_floor_forced_completion_threshold 가 현재 살아 있다고 가정하지 않는다.
- floor rebase, spatial rebase 가 지금도 동작한다고 문서화하거나, 그 전제를 바탕으로 로직을 덧붙이지 않는다.
- 현재 active-workfront 제어의 핵심은 throttling heuristic 이 아니라 remaining-work proportional control + retry loop 라는 점을 유지한다.
- Sequence != Step, global step cycle 집계, complete-pattern emission 원칙을 깨뜨리지 않는다.

## 12. 요약

파일명은 active-workfront-throttling-v3.md 이지만, 현재 구현은 과거의 throttling v3 상태기계를 유지하고 있지 않다.

현재 엔진은 다음 조합으로 수렴해 있다.

- global step cycle
- remaining-work proportional active-set control
- round-robin active workfront selection
- single-candidate retry loop
- buffer classification
- complete-pattern emission
- buffer-only rollback

따라서 이 문서를 읽을 때의 핵심 해석은 다음 한 줄이다.

- 현재 active workfront 제어는 throttling 상태기계가 아니라, 남은 작업량 비례 active-set 축소와 retry-loop 기반 local-step 형성 로직이다.