# Active Workfront Throttling v3

목표는 다음 두 가지다.

- large model + multi-workfront 조합에서 발생한 same-floor sparse endgame 정체 문제를 어떻게 해석하고 수정해 왔는지 시간 순서대로 남긴다.
- 현재 빌드 기준으로 실제 살아 있는 canonical 동작이 무엇인지 한 문서에서 이해할 수 있게 한다.

이 문서는 과거 실험과 현재 동작을 모두 담지만, 최종 해석은 **현재 빌드에서 실제로 동작하는 수렴 상태**를 기준으로 읽어야 한다.

## 1. 문제의 출발점

Simulation Mode 에서 large model + multi-workfront 조합을 돌릴 때, 특히 sparse endgame 에서 `TerminationReason::MaxIterations` 로 끝나는 문제가 반복적으로 관찰됐다.

초기 대표 사례:

- `6x24x3` 모델에서 2-workfront 는 완주했지만 6-workfront, 특히 clustered 배치에서 sparse endgame 정체가 발생했다.
- 이후 실제 UI-like `6x22x3` 배치에서도 `Scenario 2`가 `MaxIterations` 로 실패했다.

당시 공통 증상:

- sequence round 는 계속 증가하는데 local step emission 이 잘 안 일어난다.
- 마지막 층 후반부에서 여러 workfront 가 같은 floor 의 좁은 잔여 부재 집합을 동시에 노린다.
- floor lock / replan / selected-this-sequence 충돌이 반복되면서 어느 쪽도 local step 을 안정적으로 완성하지 못한다.

핵심 해석:

- 문제의 본질은 단순 locality distance 나 workfront 수 자체가 아니라 **same-floor sparse contention** 이다.
- 특히 후반부에는 “누가 더 좋은 후보를 보느냐”보다 “누가 다른 workfront 와 덜 충돌하면서 그 floor 를 마감할 수 있느냐”가 더 중요하다.

## 2. v1 단계: sparse same-floor contention 완화 실험

v1 의 목표는 workfront 수를 영구적으로 줄이는 것이 아니라, **문제가 되는 sparse same-floor round 에서만 임시로 active set 을 좁혀 local step 형성을 쉽게 만드는 것**이었다.

즉:

- early phase normal flow 는 유지한다.
- buffer 가 이미 있는 workfront 는 계속 밀어준다.
- committed floor 가 있는 workfront 는 끊지 않는다.
- 아무것도 없는 same-floor 경쟁 구간에서만 representative workfront 만 남긴다.

### 2.1 v1 preview 계산

각 eligible workfront 에 대해 아래 정보를 계산했다.

- `target_floor`
- `has_buffer`
- `has_committed_floor`
- `top_candidate_score`
- `candidate_count`
- `zone_key`

여기서 `top_candidate_score` 는 current floor 기준 가장 좋은 단일 candidate 점수이고, `candidate_count` 는 실제로 고려 가능한 floor seed 수다.

### 2.2 v1 우선순위 규칙

active set 선택 우선순위는 다음과 같았다.

1. `buffer_sequences` 가 있는 workfront 우선
2. buffer 는 없지만 `committed_floor` 가 있는 workfront 우선
3. 위 두 경우가 없을 때만 sparse same-floor contention 판단

의도는 분명했다.

- 이미 local bundle 을 쌓고 있는 workfront 를 끊지 않는다.
- floor lock 을 가진 workfront 도 중간에 쉬게 하지 않는다.
- 정말 아무도 확정 우위를 못 가진 상태에서만 경쟁을 줄인다.

### 2.3 v1 trigger

v1 은 broad throttling 을 피하고 아래 경우에만 개입했다.

- 같은 `target_floor` 그룹에 workfront 가 2개 이상 있고
- 그 그룹의 `max(candidate_count)` 가 `lower_floor_forced_completion_threshold` 이하이고
- 현재 cycle 에서 아직 local step 이 하나도 완성되지 않았을 때

즉, v1 은 threshold 기반 sparse 판단을 throttling trigger 의 핵심으로 사용했다.

### 2.4 v1 zone representative 선택

throttling 이 필요하면 각 coarse zone 에서 대표 workfront 1개만 active 로 남겼다.

zone 기준:

- `zone_width = grid.nx / 2`
- `zone_height = grid.ny / 2`
- `zone_key = (grid_x / zone_width, grid_y / zone_height)`

대표 선정 기준:

1. `top_candidate_score` 가 더 큰 workfront
2. 동점이면 `candidate_count` 가 더 큰 workfront
3. 그래도 동점이면 `wf_id` 가 더 작은 workfront

보류된 workfront 는 제거되는 것이 아니라, **그 sequence round 에서만** 임시 비활성화됐다.

### 2.5 v1 에서 얻은 교훈

v1 시기에 가장 중요했던 교훈은 “throttling 을 어디서 켜는가”가 로직 자체보다 더 중요하다는 점이었다.

주요 교훈:

- broad throttling 은 regression 을 만든다.
- focus floor 를 너무 강하게 고정하면 distributed case 를 망친다.
- committed workfront 까지 쉬게 하면 local step completion 이 더 늦어진다.
- `candidate_count` 는 sparse 판단 신호로는 유용하지만 대표 선정을 전부 대체하면 안 된다.
- zone 분할은 coarse heuristic 일 뿐이며, clustered case 를 푸는 데는 쓸 수 있어도 일반해는 아니다.

v1 검증 당시 통과한 회귀:

- 2-workfront baseline
- distributed 6-workfront
- clustered 6-workfront

하지만 이후 실제 UI-like `6x22x3` 실패를 충분히 설명하거나 안정적으로 막는 데는 부족했다.

## 3. v1 에서 v2 로 넘어간 이유

v1 은 “sparse contention 에만 좁게 개입한다”는 방향성은 맞았지만, 핵심이 zone heuristic 이라고 보기에는 한계가 드러났다.

추가 관찰을 통해 해석이 바뀌었다.

- 문제의 핵심은 coarse zone 분할 자체보다 **same-floor competition** 이었다.
- 후반부에는 representative 를 zone 별로 하나씩 남기는 것보다, 실제 현재 top candidate 충돌을 직접 줄이는 편이 더 중요했다.
- 특히 UI-like `6x22x3` 실패는 v1 의 coarse zone 대표 선정만으로는 안정적으로 제어되지 않았다.

그래서 v2 는 다음 방향으로 재설계됐다.

- threshold 기반 sparse 판정을 throttling trigger 의 핵심에서 내린다.
- same-floor 정체가 실제로 시작됐는지를 trigger 로 본다.
- zone representative heuristic 보다 `top_candidate_id` 기반 직접 충돌 회피를 우선한다.
- non-selected workfront 를 단순히 쉬게 하는 것을 넘어, buffer rollback 뒤 일반 floor selection 흐름으로 되돌린다.

## 4. v2 단계: 현재 빌드의 수렴 상태

v2 의 목표는 아래 두 가지다.

- 같은 층 경쟁이 발생하면 그 층에서 **상위 2개 workfront 만 active** 로 남기고, 나머지는 buffer rollback 후 일반 floor selection 흐름으로 돌려보낸다.
- 이 되돌림 경로에 최소 floor rebase 와 spatial anchor 보강을 넣고, 그 개입 횟수를 UI 에서 확인할 수 있게 한다.

즉:

- 문제를 same-floor competition 으로 직접 다룬다.
- bootstrap 이전에는 개입하지 않는다.
- zone representative 같은 coarse heuristic 보다, 실제 현재 score 와 top candidate 충돌 회피를 우선한다.
- 실패 시 cap 을 1로 줄여 single-WF finish mode 로 내려간다.

## 5. 현재 v2 로직

### 5.1 workfront preview 계산

각 eligible WF 에 대해 아래 정보를 계산한다.

- `target_floor`
- `has_buffer`
- `has_committed_floor`
- `top_candidate_id`
- `top_candidate_score`
- `candidate_count`
- `remaining_on_target_floor`
- `zone_key`

여기서 중요한 변화는 `top_candidate_id` 를 함께 기록한다는 점이다.

이 값은 1번 WF 와 2번 WF 가 사실상 같은 부재를 두고 경쟁하는지 빠르게 걸러내기 위해 사용한다.

### 5.2 1번 WF 선정

정렬 우선순위는 아래와 같다.

1. `has_buffer`
2. `has_committed_floor`
3. `top_candidate_score`
4. `candidate_count`
5. `wf_id`

의도:

- 이미 그 층에서 진행 중인 WF 를 우선한다.
- 이미 lock 을 가진 WF 를 끊지 않는다.
- 새로 진입하는 WF 는 진행 중인 WF 보다 뒤로 보낸다.

### 5.3 2번 WF 선정

2번 WF 는 단순히 score 2등을 고르지 않는다.

선정 순서:

1. 1번 WF 와 `top_candidate_id` 가 다르고 `zone_key` 도 다른 WF
2. 없으면 `top_candidate_id` 만 다른 WF
3. 없으면 `zone_key` 만 다른 WF

즉, 2번 WF 는 “좋은 후보”이면서 동시에 “1번 WF 와 직접 충돌하지 않는 후보”를 우선한다.

### 5.4 v2 trigger

v2 는 threshold 기반 sparse 판정을 throttling trigger 의 핵심으로 쓰지 않는다.

현재 trigger 조건:

- `stable_ids` 가 비어 있지 않다
- 같은 floor 를 target 으로 보는 WF 가 2개 이상이다
- 해당 floor 에 `remaining_on_target_floor > 0` 이다
- 현재 cycle 에서 `cycle_no_local_step_rounds > 0` 이다

즉 bootstrap 이전에는 개입하지 않고, **한 번이라도 same-cycle 정체가 보인 뒤**에만 경쟁 완화를 건다.

### 5.5 non-selected WF 처리

v2 의 가장 중요한 수정점은 여기다.

non-selected WF 는 완전 초기화하지 않는다.

현재 처리 방식:

- `buffer_sequences` 에 들어 있던 부재만 `owned_ids` 에서 제거한다
- `buffer_sequences` 를 비운다
- `planned_pattern` 을 비운다
- `committed_floor` 를 해제한다
- `last_failed_floor` 를 현재 throttled floor 로 기록한다

핵심은 **local footprint 전체를 지우지 않는 것**이다.

### 5.6 실패 시 1개로 축소

처음에는 최대 2개 WF 를 남긴다.

하지만 같은 floor 에서 정체가 계속되면:

- `sequence_installations.is_empty()` 이거나
- local step 이 추가되지 않는 round 가 이어질 때

`active_cap` 을 1로 줄인다.

즉, 2-WF cooperative finish 가 실패하면 single-WF finish 로 자동 하향된다.

## 6. rebase 보강

이번 변경의 핵심은 active throttling 을 대체하는 것이 아니라, throttling 이후 non-selected WF 가 다시 일반 흐름으로 돌아갈 때 더 나은 다음 시도를 하도록 최소 상태를 추가한 것이다.

### 6.1 WorkfrontState 에 추가된 상태

현재 `WorkfrontState` 는 기존 상태 외에 아래 정보를 가진다.

- `runtime_anchor_x`, `runtime_anchor_y`
- `rebase_cooldown_rounds`
- `floor_rebase_count`
- `spatial_rebase_count`

의도:

- workfront 의 탐색 기준점을 static start point 하나로만 두지 않는다.
- throttling 으로 reset 된 뒤에는 다음 몇 round 동안 직전 실패 floor 를 덜 우선시한다.
- scenario 종료 후에는 이 rebase 시도 횟수를 metric 으로 남긴다.

### 6.2 spatial anchor 연결 범위

runtime anchor 는 현재 아래 용도로 연결되어 있다.

- local footprint 가 비어 있는 floor 에서 거리 계산의 기준점
- floor 1 strict anchor check
- 새 element 선택 후 anchor 를 그 element 위치 쪽으로 업데이트
- throttling 으로 reset 된 WF 가 다음 시도에서 사용할 rebase anchor 설정

중요:

- active throttling 의 endgame zone selection 자체는 기존 static workfront zone 을 유지한다.
- spatial rebase 는 **candidate search locality 보정**과 reset 이후 다음 시도 anchor 설정에만 연결되어 있다.

### 6.3 floor rebase는 최소 cooldown 방식

현재 floor rebase 는 공격적인 floor migration 이 아니라, reset 이후 짧은 cooldown 동안 최근 실패 floor 를 다시 첫 우선순위로 보지 않게 만드는 수준이다.

동작:

- non-selected WF reset 시 `rebase_cooldown_rounds = 2`
- floor selection 에서 cooldown 이 남아 있으면 `last_failed_floor` 를 우선 회피
- 새 element 를 선택할 때마다 cooldown 을 1씩 감소

이 방식은 기존 canonical floor 제약을 유지하면서도, 같은 floor 재충돌 루프를 완화하기 위한 최소 개입이다.

### 6.4 reset 시 spatial rebase anchor 부여

throttled floor 에서 non-selected WF 는 기존 buffer-only rollback 뒤에 추가로 아래를 수행한다.

- selected WF 들의 current anchor 를 수집
- 그 anchor 들과 최대한 떨어진 후보 anchor 를 선택
- 해당 위치를 다음 시도용 runtime anchor 로 기록
- floor / spatial rebase event 카운터 증가

여기서도 핵심은 **footprint 전체 삭제가 아니라, 다음 탐색의 출발점만 바꾸는 것**이다.

## 7. 조건 분기 상세

현재 빌드 기준으로 v2 는 단순히 “same-floor 정체면 throttling” 한 줄로 끝나지 않는다. 실제로는 아래 분기들이 함께 작동한다.

### 7.1 active throttling trigger 와 floor threshold 는 다른 분기다

same-floor 경쟁 완화의 직접 trigger 는 아래 조건 조합이다.

- `stable_ids` 가 비어 있지 않다
- 같은 floor 를 target 으로 보는 WF 가 2개 이상이다
- 해당 floor 에 `remaining_on_target_floor > 0` 이다
- 현재 cycle 에서 `cycle_no_local_step_rounds > 0` 이다

즉, active throttling 은 **bootstrap 이후 same-cycle 정체가 시작됐는가**를 보고 켜진다.

반면 `lower_floor_forced_completion_threshold` 는 이 trigger 에 직접 쓰이지 않는다. 이 값은 아래의 floor gate / forced completion 분기에서 사용된다.

### 7.2 상층 후보 차단 분기

`lower_floor_forced_completion_threshold` 는 상층 기둥 후보를 차단하는 hard gate 로 사용된다.

동작:

- 어떤 상층 후보를 보려면 먼저 하층 설치율 조건을 본다.
- 하층 전체 기둥이 아직 다 설치되지 않았고,
- 하층 잔여 기둥 수가 threshold 이하이면,
- 상층 신규 후보는 막고 하층 마감을 우선한다.

즉, 이 값은 “throttling trigger” 가 아니라 **상층 진입 차단 분기**다.

### 7.3 floor eligibility 분기

uncommitted 상태에서 특정 floor 가 신규 타깃 floor 로 선택 가능한지도 같은 threshold 영향을 받는다.

동작:

- 하층 완료율이 `lower_floor_completion_ratio_threshold` 미만이면 상층 진입 불가
- 하층 잔여 기둥 수가 `1..=lower_floor_forced_completion_threshold` 범위면 상층 진입 불가
- 이 두 조건을 통과한 floor 만 신규 target floor 후보가 된다

즉, floor selection 은 단순 점수 경쟁이 아니라 **completion ratio gate + forced completion gate** 를 지난 floor 들 사이에서만 일어난다.

### 7.4 말단 구간 relaxed search 분기

`remaining_on_target_floor <= lower_floor_forced_completion_threshold` 가 되면, target floor 말단 구간으로 보고 relaxed candidate search 를 허용한다.

의미:

- 기본 locality 제약으로 후보가 비어 있더라도
- 같은 target floor 를 마감하기 위한 추가 후보를 더 넓게 보게 된다

즉, 이 값은 한편으로는 상층 진입을 막는 hard gate 이면서, 다른 한편으로는 **현재 floor 말단 마감 시 locality 완화를 허용하는 분기**이기도 하다.

### 7.5 local step 방출 분기

local buffer 가 complete pattern 이 되어도, 현재 floor 의 잔여 부재 수가 threshold 보다 크면 바로 방출되지 않는다.

동작:

- cycle 내 committed buffer 들을 기준으로 current floor 잔여 부재 수를 계산
- `remaining_on_floor > lower_floor_forced_completion_threshold` 이면 현재 local step 방출을 막음
- threshold 이하로 내려오면 해당 floor 마감 구간으로 보고 step 방출이 가능해진다

즉, 이 값은 후보 생성 전단의 gate 뿐 아니라 **step emission 시점 제어**에도 참여한다.

### 7.6 현재 빌드 기준 한 줄 요약

- active throttling trigger: same-floor 정체 감지
- lower floor forced completion threshold: 하층 우선 마감 강제 + 상층 진입 차단 + target floor 말단 완화 + local step 방출 조건

따라서 `lower_floor_forced_completion_threshold = 10` 을 “active throttling 을 켜는 숫자”로 이해하면 틀리고, **여러 floor-control 분기에 재사용되는 핵심 hard threshold** 로 이해해야 한다.

## 8. UI 연결

이번 단계부터 simulation result panel 은 기존 종료/스텝/설치 수 외에 아래 진단값도 표시한다.

- `Throttle Events`
- `Floor Rebase Events`
- `Spatial Rebase Events`

이 값들은 scenario 단위 집계이며, 현재 목적은 사용자와 개발자가 rebase 보강이 실제로 몇 번 개입했는지 빠르게 보는 것이다.

이 진단값은 현재 scenario ranking 변경용 신호가 아니라, 시뮬레이션 결과 해석과 디버깅을 위한 표시 값이다.

## 9. 가장 중요한 갓챠

### 9.1 broad throttling 은 regression 을 만든다

가장 큰 갓챠는 throttling 자체보다 **언제 켜는지**였다.

너무 넓게 켜면:

- distributed case completion 이 무너진다.
- floor 1 / floor 2 확장이 비정상적으로 느려진다.
- 결과적으로 missing elements 가 더 늘어난다.

즉, 이 로직은 성능 최적화가 아니라 **late sparse contention workaround** 로 다뤄야 한다.

### 9.2 bootstrap 이전 개입 금지

same-floor competition 제어는 초기 독립 구조 형성 전에 켜면 안 된다.

bootstrap 이전에 non-selected WF 를 reset 하면:

- 아직 정상적으로 출발해야 할 WF 까지 끊기고
- 초기 floor 1 확장이 무너지고
- 심하면 설치 수가 0까지 떨어질 수 있다.

따라서 `!stable_ids.is_empty()` 조건은 필수 안전장치다.

### 9.3 rollback 과 reset 은 같은 의미가 아니었다

이번 작업에서 가장 큰 실수는 non-selected WF 를 reset 할 때 `owned_ids.clear()` 로 전체 footprint 를 날린 것이었다.

올바른 의미는:

- 현재 buffer 만 해제한다
- 이미 구축된 로컬 footprint 전체는 보존한다

즉, competition control 은 “현재 들고 있던 시도”를 롤백하는 것이지, WF 의 지역 문맥 전체를 삭제하는 것이 아니다.

### 9.4 committed workfront 는 쉽게 끊으면 안 된다

이미 buffer 나 committed floor 가 있는 workfront 를 쉬게 하면:

- local step completion 이 늦어지고
- rollback / replan 이 더 잦아지고
- sparse endgame 이 더 길어진다.

그래서 현재 정렬 우선순위도 `has_buffer`, `has_committed_floor` 를 앞에 둔다.

### 9.5 spatial rebase 의 적용 범위는 보수적으로 유지해야 한다

runtime anchor 를 active throttling representative selection core 까지 직접 밀어 넣으면 UI-like 재현 테스트가 다시 `MaxIterations` 로 회귀했다.

따라서 현재 수렴 상태는 다음과 같다.

- candidate search locality 와 reset 이후 다음 시도 anchor 보정에는 runtime anchor 를 쓴다.
- active throttling selection core 는 static workfront zone 기준을 유지한다.

## 10. 검증의 시간 순서

### 10.1 v1 단계 검증

아래 회귀 테스트를 통과했다.

- 2-workfront baseline
- distributed 6-workfront
- clustered 6-workfront

실행한 검증 명령:

```powershell
cargo test --manifest-path src/rust/Cargo.toml test_simulation_completes_6x24x3_with_two_workfronts -- --nocapture
cargo test --manifest-path src/rust/Cargo.toml test_simulation_completes_6x24x3_with_six_workfronts -- --nocapture
cargo test --manifest-path src/rust/Cargo.toml test_simulation_completes_6x24x3_with_six_clustered_workfronts -- --nocapture
cargo build --release --manifest-path src/rust/Cargo.toml
```

### 10.2 v2 단계 검증

현재 직접 검증한 케이스:

- `test_simulation_completes_6x22x3_with_ui_case_workfronts_scenario2_seed`

실행 명령:

```powershell
cargo test --manifest-path src/rust/Cargo.toml test_simulation_completes_6x22x3_with_ui_case_workfronts_scenario2_seed -- --nocapture
```

결과:

- 통과
- rebase 상태 추가 + UI metrics 노출 이후에도 통과 확인
- `cargo build --release` 성공

## 11. 현재 code truth

현재 빌드 기준 code truth 는 아래 한 줄로 요약된다.

- **active throttling 유지 + buffer-only rollback + minimal floor rebase + runtime-anchor 기반 spatial rebase + UI diagnostics**

따라서 이 문서를 읽고 active throttling 제거를 전제로 후속 수정을 하면 안 된다.

## 12. 요약

v1 에서 얻은 결론은 “same-floor sparse contention 에만 아주 좁게 개입해야 한다”는 것이었고, v2 에서 도달한 현재 상태는 “bootstrap 이후 same-floor 정체가 시작되면 그 floor 에서 score 가 좋은 WF 2개만 유지하고 나머지는 buffer rollback 후 일반 floor selection 흐름으로 되돌리되, 그 경로에 minimal floor rebase 와 spatial anchor 보강, 그리고 UI diagnostics 를 추가한다”는 것이다.
