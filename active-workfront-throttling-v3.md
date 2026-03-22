# Active Workfront Throttling v3

이 문서는 현재 `sim_engine.rs` 에서 실제로 동작하는 active workfront 제어 방식을 기술한다. 파일명은 유지하지만 내용은 현재 코드 기준이다.

## 1. 한 줄 요약

현재 코드는 별도의 throttling 상태기계를 사용하지 않는다.

- 각 global step cycle 에서 eligible workfront 집합을 구한다.
- 남은 미설치 부재 수와 `workfront 수 × 5` 를 비교해 이번 round 에 active 로 돌릴 workfront 수를 계산한다.
- eligible 집합에서 round-robin 으로 active set 을 고른다.
- 각 active workfront 는 round 당 최대 1개 부재만 선택한다.
- local buffer 가 complete pattern + stability pass 가 될 때만 `LocalStep` 을 만들고, cycle 종료 시 global `SimStep` 으로 병합한다.

## 2. active workfront 제어의 목적

- 후반부로 갈수록 모든 workfront 를 계속 동시에 돌리지 않고, 남은 작업량에 맞게 active 수를 줄여 충돌과 공회전을 줄인다.
- 각 workfront 의 local buffer 기반 패턴 완성 흐름과 Sequence ≠ Step 원칙은 깨뜨리지 않는다.

## 3. active workfront 는 어떻게 정해지는가

### 3.1 eligible workfront

한 cycle 의 각 round 시작 시, 이미 그 cycle 안에서 `LocalStep` 생성에 성공한 workfront 는 제외된다.

```rust
// sim_engine.rs
let eligible_wfs: Vec<&SimWorkfront> = workfronts
    .iter()
    .filter(|wf| !cycle_completed_wf.contains(&wf.id))
    .collect();
```

### 3.2 active count 계산

eligible workfront 전체를 매 round 전부 돌리지 않는다. 남은 미설치 부재 수를 `workfront 수 × 5` (capacity baseline) 와 비교해 이번 round 의 active workfront 수를 결정한다.

각 workfront 가 한 local step 에서 생산할 수 있는 부재 수 상한이 약 5개이므로 이 값을 baseline 으로 삼는다.

```rust
// sim_engine.rs L1297–1310
// WF proportional control: scale active count by remaining work.
// Each workfront can produce up to ~5 members per local step, so we treat
// (workfronts × 5) as the capacity baseline for proportional scaling.
let current_remaining = total_elements.saturating_sub(committed_ids.len());
let wf_count = workfronts.len();
let capacity_baseline = wf_count * 5;
let active_count = if current_remaining == 0 {
    1
} else if current_remaining >= capacity_baseline {
    wf_count
} else {
    // Round up so we don't retire workfronts too aggressively
    std::cmp::max(1, (current_remaining * wf_count + capacity_baseline - 1) / capacity_baseline)
};
```

결정 규칙 요약:

| 조건 | active_count |
|------|-------------|
| `remaining == 0` | 1 (강제) |
| `remaining >= wf_count × 5` | `wf_count` (전체) |
| `remaining < wf_count × 5` | 비례 ceiling, 최소 1 |

즉, 남은 부재가 workfront 당 5개 이상 있으면 모든 workfront 를 활성화하고, 그 미만이면 남은 작업량에 비례해 자연스럽게 줄어든다.

### 3.3 active workfront 선택 방식

active 집합은 eligible 집합에서 round-robin 으로 선택한다.

```rust
// sim_engine.rs L1314–1322
let start_index = (cycle_round_index - 1) % eligible_wfs.len();
let active_wfs: Vec<&SimWorkfront> = eligible_wfs
    .iter()
    .cycle()
    .skip(start_index)
    .take(active_count.min(eligible_wfs.len()))
    .copied()
    .collect();
```

라운드가 진행될수록 `start_index` 가 회전하므로 특정 workfront 가 우선권을 독점하지 않는다.

## 4. round 내부 실제 흐름

### 4.1 bootstrap 이전

안정 구조가 전혀 없으면 bootstrap 경로를 사용한다.

- 조건: `stable_ids.is_empty() && cycle_local_steps.is_empty()`
- workfront anchor 근처 bootstrap bundle 후보를 만들고 w1/w2/w3 점수로 weighted sampling 한다.
- 선택된 bootstrap pattern 전체를 그 workfront buffer 에 넣는다.
- bootstrap 단계에서는 active count 제어보다 bundle 선택이 핵심이다.

### 4.2 bootstrap 이후

각 active workfront 가 증분 후보를 single-candidate retry loop 로 하나씩 시도한다.

1. round 시작 시 `allowed_floors` 를 먼저 계산한다. `committed_floor` 가 있으면 그 층만 허용한다.
2. candidate pool 에서 floor gate, buffer mask, locality, structural possibility 를 통과한 후보만 남긴다.
3. 점수 기반 weighted random choice 로 후보 1개를 뽑는다.
4. 그 후보가 현재 buffer 를 실제로 진전시키는지 검사한다. 진전시키지 못하면 제외하고 다음 후보를 시도한다.
5. 통과한 후보가 있으면 그 1개만 이번 round 설치 후보가 된다.
6. complete pattern + stability pass 일 때만 `LocalStep` 을 방출한다.

## 5. rollback

rollback 은 buffer-only 이며 rebase 상태를 심지 않는다.

발생 조건 3가지:

| 조건 | trace 이벤트 |
|------|-------------|
| buffer 가 `StepBufferDecision::Invalid` | `sim.wf.rollback` (reason: `invalid_pattern`) |
| complete 패턴이 stability fail → `EmitResult::Infeasible` | `sim.wf.infeasible_rollback` |
| `committed_floor` 상태에서 유효 후보 없음 | `sim.wf.rollback` (reason: no candidates) |

처리 방식 (세 경우 모두 동일):

```rust
for eid in &rollback_ids { state.owned_ids.remove(eid); }
state.buffer_sequences.clear();
state.committed_floor = None;
cycle_rollback_count += 1;
```

rollback 횟수는 cycle 단위로 `cycle_rollback_count` 에 누적되고 cycle 종료 trace 에 기록된다.

## 6. metrics 의 throttle/rebase 필드는 placeholder

`ScenarioMetrics` 에 `throttle_events`, `floor_rebase_events`, `spatial_rebase_events` 필드가 남아 있으나, 현재 `sim_engine.rs` 에서는 모두 `0` 으로만 채운다. 현재 활성 기능이 아니다.

## 7. 현재 구현에 없는 것

- `FloorThrottleState` / `FloorThrottleMode` (Observe, CapTwo, CapOne, Cooldown)
- `WorkfrontActivationInfo`, `FloorCongestionInfo`
- zone representative 선별 로직
- `planned_pattern`, `last_failed_floor`
- `runtime_anchor_x`, `runtime_anchor_y`
- `rebase_cooldown_rounds`
- `floor_rebase_count`, `spatial_rebase_count`
- `lower_floor_forced_completion_threshold`
- floor rebase, spatial rebase
- forced completion 기반 하층 마감 우선

## 8. 후속 수정 시 주의사항

- `capacity_baseline = wf_count * 5` 수식을 변경할 때는 후반부 축소 기울기가 달라지므로 시나리오 품질을 함께 검증한다.
- `active_count` 와 `eligible_wfs` 계산은 반드시 candidate collection 이전에 완료되어야 한다.
- Sequence ≠ Step 원칙, global step cycle 집계, complete-pattern emission 원칙을 깨뜨리지 않는다.
- floor rebase, spatial rebase, CapTwo/CapOne 상태기계를 살아있다고 전제하고 코드를 추가하지 않는다.