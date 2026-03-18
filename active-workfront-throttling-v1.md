# Active Workfront Throttling v1

이 문서는 Simulation Mode에서 large model + multi-workfront 조합에서 발생한 sparse endgame 정체 문제와, 이를 완화하기 위해 도입한 active workfront throttling v1 로직을 정리한 작업 메모다.

## 배경

`6x24x3` 같은 큰 모델에서 workfront가 2개일 때는 완주했지만, 같은 모델에 workfront를 6개 두면 특히 clustered 배치에서 마지막 구간이 `MaxIterations`로 끝나는 문제가 있었다.

당시 관찰된 특징:

- distributed 6-workfront는 상대적으로 양호했지만 clustered 6-workfront는 sparse endgame에서 자주 정체됐다.
- 마지막 층 후반부에는 남은 부재 수가 적어지는데, 여러 workfront가 동시에 같은 floor의 seed를 두고 경쟁하면서 어느 한쪽도 local step을 안정적으로 완성하지 못하는 상황이 발생했다.
- 문제의 핵심은 단순 locality distance가 아니라, **same-floor sparse contention**이었다.

## 문제 정의

다음 조건이 동시에 생기면 엔진이 비효율적으로 흔들렸다.

1. 같은 target floor를 노리는 eligible workfront가 여러 개 존재
2. 각 workfront의 candidate 수가 이미 sparse 수준으로 줄어듦
3. 아직 어떤 workfront도 local step을 확정하지 못함
4. 동일 cycle 안에서 모든 workfront가 동시에 seed를 잡으려다 buffer 축적이 분산됨

이 상태가 길어지면:

- sequence round는 계속 증가
- local step emission은 잘 안 일어남
- floor lock / plan replan / selected_this_sequence 충돌이 반복
- 최종적으로 `TerminationReason::MaxIterations`로 종료될 수 있다.

## v1 목표

목표는 workfront 전체를 영구적으로 줄이는 것이 아니라, **문제가 되는 sparse same-floor round에서만 임시로 active set을 좁혀** local step 형성을 쉽게 만드는 것이다.

즉:

- early phase normal flow는 유지
- buffer가 이미 있는 workfront는 계속 밀어줌
- committed floor가 있는 workfront는 끊지 않음
- 아무것도 없는 same-floor 경쟁 구간에서만 대표 workfront만 남김

## 적용 로직

적용 위치는 [assyplan/src/rust/src/sim_engine.rs](assyplan/src/rust/src/sim_engine.rs#L1235) 의 `select_active_workfronts()`다.

보조 구조:

- [WorkfrontActivationInfo](assyplan/src/rust/src/sim_engine.rs#L76)
- [preview_workfront_activation_info()](assyplan/src/rust/src/sim_engine.rs#L1044)

### 1. workfront별 preview 계산

각 eligible workfront에 대해 아래 정보를 미리 계산한다.

- `target_floor`
- `has_buffer`
- `has_committed_floor`
- `top_candidate_score`
- `candidate_count`
- `zone_key`

여기서 `top_candidate_score`는 current floor 기준 가장 좋은 단일 candidate 점수이고, `candidate_count`는 현재 조건에서 실제로 고려 가능한 floor seed 수다.

### 2. 우선순위 규칙

active set 선택 우선순위는 다음과 같다.

1. `buffer_sequences`가 있는 workfront 우선
2. buffer는 없지만 `committed_floor`가 있는 workfront 우선
3. 위 두 경우가 없을 때만 sparse same-floor contention 판단

이 순서는 다음 의도를 가진다.

- 이미 local bundle을 쌓고 있는 workfront를 끊지 않는다.
- floor lock을 가진 workfront도 중간에 굳이 쉬게 하지 않는다.
- 정말 아무도 확정 우위를 못 가진 상태에서만 경쟁을 줄인다.

### 3. sparse same-floor contention 판정

throttling은 broad하게 걸지 않고, 아래 경우에만 건다.

- 같은 `target_floor` 그룹에 workfront가 2개 이상 있고
- 그 그룹의 `max(candidate_count)`가 `lower_floor_forced_completion_threshold` 이하일 때
- 그리고 현재 cycle에서 아직 local step이 하나도 완성되지 않았을 때

즉, early/mid phase에는 거의 개입하지 않고, local step 형성 전의 sparse endgame contention에만 개입한다.

### 4. zone representative 선택

throttling이 필요하면 각 coarse zone에서 대표 workfront 1개만 active로 남긴다.

zone 기준:

- `zone_width = grid.nx / 2`
- `zone_height = grid.ny / 2`
- `zone_key = (grid_x / zone_width, grid_y / zone_height)`

대표 선정 기준:

1. `top_candidate_score`가 더 큰 workfront
2. 동점이면 `candidate_count`가 더 큰 workfront
3. 그래도 동점이면 `wf_id`가 더 작은 workfront

보류된 workfront는 제거되는 것이 아니라, **그 sequence round에서만** 임시 비활성화된다.

## 왜 이 형태로 좁혔는가

처음에는 throttling을 더 넓게 적용했는데, 그 버전은 distributed 6-workfront 같은 정상 케이스도 크게 악화시켰다.

실패 원인:

- active set을 너무 일찍 줄이면 정상적인 병렬 확장까지 막힌다.
- floor focus를 너무 강하게 걸면 여러 floor가 자연스럽게 병행되던 흐름이 깨진다.
- committed workfront까지 줄이면 buffer completion이 오히려 더 늦어진다.

그래서 v1은 아래 원칙으로 다시 좁혔다.

- buffer-holder는 무조건 살린다.
- committed_floor holder도 살린다.
- current cycle에 local step이 이미 나온 뒤에는 throttling하지 않는다.
- same-floor sparse contention에만 제한적으로 개입한다.

## 갓챠

### 1. broad throttling은 regression을 만든다

가장 큰 갓챠는 throttling 자체보다 **언제 켜는지**였다.

너무 넓게 켜면:

- distributed 6-workfront completion이 무너진다.
- floor 1 / floor 2 확장이 비정상적으로 느려진다.
- 결과적으로 missing elements가 더 늘어난다.

즉, 이 로직은 성능 최적화가 아니라 **late sparse contention workaround**에 가깝게 다뤄야 한다.

### 2. focus floor 고정은 생각보다 위험하다

처음에는 priority가 가장 높은 floor만 남기고 나머지 floor를 사실상 쉬게 하는 방향을 시도했는데, 이건 distributed case를 망가뜨렸다.

v1은 floor 전체를 잠그지 않고, **같은 floor 안에서만** 대표를 줄이는 쪽으로 조정했다.

### 3. committed workfront까지 쉬게 하면 안 된다

이미 buffer나 committed floor가 있는 workfront를 쉬게 하면:

- local step completion이 늦어지고
- lock rollback/replan이 더 잦아지고
- sparse endgame이 더 길어진다.

따라서 v1은 committed workfront throttling을 하지 않는다.

### 4. candidate_count는 sparse 판단용이지 score 대체값이 아니다

`candidate_count`는 이 floor가 sparse한지 보는 신호로는 유용하지만, 대표 선정을 전부 candidate 수로 해버리면 locality나 structural quality가 무시된다.

그래서 대표 선정 1순위는 `top_candidate_score`로 두고, `candidate_count`는 보조 tie-breaker로만 쓴다.

### 5. zone 분할은 coarse heuristic이다

현재 zone은 `nx/2`, `ny/2`로 나눈 아주 거친 heuristic이다.

이 값은:

- clustered case를 푸는 데는 충분했지만
- 다른 grid aspect ratio에서는 너무 거칠거나 너무 세밀할 수 있다.

다음 작업에서 zone granularity를 튜닝할 여지는 있다.

## 검증 결과

아래 회귀 테스트를 통과했다.

- [2-workfront baseline](assyplan/src/rust/src/sim_engine.rs#L4162)
- [distributed 6-workfront](assyplan/src/rust/src/sim_engine.rs#L4196)
- [clustered 6-workfront](assyplan/src/rust/src/sim_engine.rs#L4239)

실행한 검증 명령:

```powershell
cargo test --manifest-path src/rust/Cargo.toml test_simulation_completes_6x24x3_with_two_workfronts -- --nocapture
cargo test --manifest-path src/rust/Cargo.toml test_simulation_completes_6x24x3_with_six_workfronts -- --nocapture
cargo test --manifest-path src/rust/Cargo.toml test_simulation_completes_6x24x3_with_six_clustered_workfronts -- --nocapture
cargo build --release --manifest-path src/rust/Cargo.toml
```

## 다음 작업 때 보면 좋은 포인트

다음 단계에서 더 다듬고 싶다면 우선순위는 아래 정도가 적절하다.

1. zone granularity를 grid shape에 맞게 더 안정적으로 계산할지 검토
2. sparse contention 판정을 `candidate_count` 외에 remaining-on-floor 정보와 결합할지 검토
3. representative selection을 단일 score가 아니라 pattern completion 가능성까지 반영할지 검토
4. throttling 발동/해제 횟수를 metrics/debug export에 남길지 검토

## 요약

v1의 핵심은 다음 한 줄이다.

**buffer/commit이 없는 same-floor sparse 경쟁 구간에서만 representative workfront를 남기고, 나머지는 그 round에서 잠깐 쉬게 해서 local step completion을 유도한다.**