# Active Workfront Throttling v2

이 문서는 현재 `sim_engine.rs`에 반영된 active workfront 경쟁 완화 로직의 v2 상태를 기록한다.

v1 문서는 과거 실험 기록으로 유지하고, 이 문서는 **현재 동작하는 단순화 버전**만 설명한다.

## 배경

문제는 large model + multi-workfront 조합에서 같은 층 후반부에 여러 workfront가 동시에 좁은 미설치 부재 집합을 두고 경쟁하면서 발생했다.

주요 증상:

- workfront 수가 늘어날수록 동일 floor 후반부 경쟁이 심해진다.
- cycle 내에서 sequence round는 계속 증가하지만 local step이 잘 안 나온다.
- floor completion pressure와 locality 완화가 함께 작동하면, 여러 WF가 같은 잔여 후보를 함께 보게 된다.
- 실제 UI-like `6x22x3` 배치에서 `Scenario 2`가 `MaxIterations`로 실패했다.

핵심 해석:

- 문제의 본질은 zone heuristic 자체보다 **same-floor competition**이다.
- 특히 후반부에는 "누가 더 좋은 후보를 갖는가"보다 "누가 다른 WF와 덜 충돌하면서 그 층을 마감할 수 있는가"가 더 중요하다.

## v2 목표

v2의 목표는 복잡한 cluster throttling이나 persistent halving을 버리고, 아래 한 가지에 집중하는 것이다.

- 같은 층 경쟁이 발생하면 그 층에서 **상위 2개 WF만 active**로 남기고, 나머지는 buffer rollback 후 일반 floor selection 흐름으로 돌려보낸다.

즉:

- 문제를 same-floor competition으로 직접 다룬다.
- bootstrap 이전에는 개입하지 않는다.
- zone representative 같은 coarse heuristic보다, 실제 현재 score와 top candidate 충돌 회피를 우선한다.
- 실패 시 cap을 1로 줄여 single-WF finish mode로 내려간다.

## 적용 위치

주요 구조 및 함수:

- `WorkfrontActivationInfo`: [src/rust/src/sim_engine.rs](src/rust/src/sim_engine.rs#L76)
- `choose_endgame_workfront_ids()`: [src/rust/src/sim_engine.rs](src/rust/src/sim_engine.rs#L1268)
- `select_active_workfronts()`: [src/rust/src/sim_engine.rs](src/rust/src/sim_engine.rs#L1303)
- 비선정 WF rollback 적용: [src/rust/src/sim_engine.rs](src/rust/src/sim_engine.rs#L2329)

## v2 로직

### 1. workfront preview 계산

각 eligible WF에 대해 아래 정보를 계산한다.

- `target_floor`
- `has_buffer`
- `has_committed_floor`
- `top_candidate_id`
- `top_candidate_score`
- `candidate_count`
- `remaining_on_target_floor`
- `zone_key`

여기서 중요한 변화는 `top_candidate_id`를 함께 기록한다는 점이다.

이 값은 1번 WF와 2번 WF가 사실상 같은 부재를 두고 경쟁하는지 빠르게 걸러내기 위해 사용한다.

### 2. 1번 WF 선정

정렬 우선순위는 아래와 같다.

1. `has_buffer`
2. `has_committed_floor`
3. `top_candidate_score`
4. `candidate_count`
5. `wf_id`

의도:

- 이미 그 층에서 진행 중인 WF를 우선한다.
- 이미 lock을 가진 WF를 끊지 않는다.
- 새로 진입하는 WF는 진행 중인 WF보다 뒤로 보낸다.

### 3. 2번 WF 선정

2번 WF는 단순히 score 2등을 고르지 않는다.

선정 순서:

1. 1번 WF와 `top_candidate_id`가 다르고 `zone_key`도 다른 WF
2. 없으면 `top_candidate_id`만 다른 WF
3. 없으면 `zone_key`만 다른 WF

즉, 2번 WF는 "좋은 후보"이면서 동시에 "1번 WF와 직접 충돌하지 않는 후보"를 우선한다.

### 4. trigger 조건

v2는 threshold 기반 sparse 판정을 핵심 trigger로 쓰지 않는다.

현재 trigger 조건:

- `stable_ids`가 비어 있지 않다
- 같은 floor를 target으로 보는 WF가 2개 이상이다
- 해당 floor에 `remaining_on_target_floor > 0` 이다
- 현재 cycle에서 `cycle_no_local_step_rounds > 0` 이다

즉 bootstrap 이전에는 개입하지 않고, **한 번이라도 same-cycle 정체가 보인 뒤**에만 경쟁 완화를 건다.

이 조건은 [src/rust/src/sim_engine.rs](src/rust/src/sim_engine.rs#L1410) 부근에 있다.

### 5. 비선정 WF 처리

v2에서 가장 중요한 수정점은 여기다.

비선정 WF는 완전 초기화하지 않는다.

현재 처리 방식:

- `buffer_sequences`에 들어 있던 부재만 `owned_ids`에서 제거한다
- `buffer_sequences`를 비운다
- `planned_pattern`을 비운다
- `committed_floor`를 해제한다
- `last_failed_floor`를 현재 throttled floor로 기록한다

핵심은 **local footprint 전체를 지우지 않는 것**이다.

이 부분은 [src/rust/src/sim_engine.rs](src/rust/src/sim_engine.rs#L2331) 에 반영되어 있다.

### 6. 실패 시 1개로 축소

처음에는 최대 2개 WF를 남긴다.

하지만 같은 floor에서 정체가 계속되면:

- `sequence_installations.is_empty()` 이거나
- local step이 추가되지 않는 round가 이어질 때

`active_cap`을 1로 줄인다.

즉, 2-WF cooperative finish가 실패하면 single-WF finish로 자동 하향된다.

## v2에서 버린 것

다음 실험 로직들은 현재 버전 기준 핵심에서 제외했다.

- floor+zone cluster 기반 persistent throttling
- broad same-floor throttling
- candidate-count threshold를 주된 trigger로 쓰는 방식
- 비선정 WF의 `owned_ids` 전체 삭제

제거 이유:

- 로직이 과도하게 복잡해졌다.
- UI-like 실재 실패 케이스를 충분히 안정적으로 해결하지 못했다.
- 특히 `owned_ids.clear()`는 local footprint를 잃게 만들어 진행을 크게 망가뜨렸다.

## 가장 중요한 갓챠

### 1. bootstrap 이전 개입 금지

same-floor competition 제어는 초기 독립 구조 형성 전에 켜면 안 된다.

bootstrap 이전에 비선정 WF를 reset하면:

- 아직 정상적으로 출발해야 할 WF까지 끊기고
- 초기 floor 1 확장이 무너지고
- 심하면 설치 수가 0까지 떨어질 수 있다.

따라서 `!stable_ids.is_empty()` 조건은 필수 안전장치다.

### 2. rollback과 reset은 같은 의미가 아니었다

이번 작업에서 가장 큰 실수는 비선정 WF를 reset할 때 `owned_ids.clear()`로 전체 footprint를 날린 것이었다.

올바른 의미는:

- 현재 buffer만 해제한다
- 이미 구축된 로컬 footprint 전체는 보존한다

즉, competition control은 "현재 들고 있던 시도"를 롤백하는 것이지, WF의 지역 문맥 전체를 삭제하는 것이 아니다.

### 3. threshold는 보조일 뿐 본질이 아니다

현재 문제는 threshold 10이라는 숫자보다, same-floor 정체가 발생했을 때 여러 WF가 계속 동시에 들어가는 구조였다.

그래서 v2는 threshold 숫자 자체보다:

- stable structure가 이미 있는지
- 같은 floor 경쟁이 실제로 발생했는지
- local step 정체가 시작됐는지

를 더 중요한 판단 기준으로 본다.

## 현재 검증 상태

현재 직접 검증한 케이스:

- `test_simulation_completes_6x22x3_with_ui_case_workfronts_scenario2_seed`

실행 명령:

```powershell
cargo test --manifest-path src/rust/Cargo.toml test_simulation_completes_6x22x3_with_ui_case_workfronts_scenario2_seed -- --nocapture
```

결과:

- 통과

추가 관찰:

- 실제 앱에서 `6x22` / `2 floors` / `12 workfronts` 배치로도 성공 사례가 확인됐다.
- 배치는 y축 양 끝에 workfront를 6개씩 둔 형태였고, `scenario 1`이 `736 / 736` 설치로 완료됐다.

이 반례가 의미하는 것:

- 문제를 단순히 "WF 개수가 많으면 실패한다"로 해석하면 안 된다.
- 더 중요한 변수는 **동일 시점에 여러 WF가 같은 floor의 같은 후보 집합으로 수렴하는가**이다.
- 즉, 핵심 문제는 high WF count 자체보다 **same-floor candidate overlap**과 **배치에 따른 경쟁 구조**다.

현재 v2 해석:

- WF 수가 많아도 시작점이 넓게 분산되면 각 WF가 서로 다른 로컬 문맥을 따라가며 성공할 수 있다.
- 반대로 WF 수가 더 적어도 특정 배치에서는 target floor와 top candidate가 좁은 구역에 몰리면서 실패할 수 있다.
- 따라서 v2 로직의 목적은 "WF 수 줄이기"가 아니라 "같은 floor에서 실제로 충돌하는 WF 수 줄이기"로 이해해야 한다.

주의:

- 이 문서 시점에서는 UI-like 재현 케이스를 우선 해결했고,
- 기존 `6x24x3` 회귀 세트 전체는 아직 다시 재검증하지 않았다.

## 다음 작업 메모

다음으로 확인할 우선순위:

1. `6x24x3` 2-WF / 6-WF / clustered 6-WF 회귀 재검증
2. trigger에서 `cycle_no_local_step_rounds > 0`만으로 충분한지 점검
3. 실패 케이스와 성공 케이스의 `top_candidate_id` / `target_floor` overlap 차이 비교
4. 필요하면 2번 WF 선정에 complete-plan 가능성 신호를 추가
5. 문서와 실제 코드가 계속 일치하는지 유지

## 요약

v2의 핵심은 다음 한 줄이다.

**bootstrap 이후 same-floor 경쟁 정체가 시작되면, 그 floor에서 score가 좋은 WF 2개만 유지하고 나머지는 buffer rollback 후 일반 floor selection 흐름으로 되돌린다. 그래도 실패하면 1개로 줄여 finish를 시도한다.**