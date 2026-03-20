# SKILLS.md - Development Phase 3 Canonical Guide

이 문서는 AssyPlan 개발 3단계(Simulation Mode)의 현재 구현 기준을 요약한 실무용 가이드다.
정본 우선순위는 다음과 같다.

1. `.github/copilot-instructions.md`
2. `AGENTS.md`
3. `devplandoc.md`
4. 이 문서
5. 실제 코드

## 1. Phase 3 목표

Phase 3의 핵심은 grid 기반 전체 구조요소 풀을 생성한 뒤, multi-workfront 환경에서 `Sequence != Step` 원칙을 유지하는 시뮬레이션을 수행하는 것이다.

- Grid 설정으로 전체 node/element pool 생성
- Workfront 선택 기반 시뮬레이션 실행
- Monte-Carlo + weighted sampling + retry loop 조합으로 시나리오 생성
- 패턴 기반 안정 조건을 만족할 때만 Step 생성
- 결과를 3D View, scenario table, metrics chart, debug export로 검토

## 2. 핵심 불변식

- 모든 ID는 1-based다.
- Node ID는 `z -> x -> y` 오름차순 기준이다.
- Sequence는 단위 시간 라운드다.
- Step은 개별 부재가 아니라 패턴 기반 안정 단위다.
- 같은 round 동시 설치는 동일 sequence 번호를 공유한다.
- 정상 결과에서 Step 수는 Sequence 수보다 작아야 한다.
- `Col -> Col -> Col`, `Col -> Col -> Col -> Girder`는 금지 패턴이다.
- 기존 구조에 연결 가능한 증분 후보가 있으면 독립 bootstrap보다 우선한다.

## 3. 현재 기본값

코드 기준 현재 기본값은 아래와 같다.

- `GridConfig`: `nx=4`, `ny=8`, `nz=3`, `dx=6000`, `dy=6000`, `dz=4000`
- `upper_floor_threshold = 0.3`
- `lower_floor_completion_ratio = 0.5`
- `sim_weights = (0.5, 0.3, 0.2)`
- `sim_scenario_count = 2`
- `sim_nav_sequence_mode = false`
- `sim_view_is_model = false`
- `sim_trace_write_jsonl = false`

문서를 수정할 때 예전 값인 `ny=4`, `forced_completion=5`, `scenario_count=100` 같은 설명이 남아 있지 않은지 먼저 확인한다.

## 4. 주요 모듈

### `src/rust/src/sim_grid.rs`

역할:

- grid 설정으로 전체 node/element pool 생성
- column/girder 및 floor 인덱스 계산
- element/node lookup 인덱스 제공

실무 포인트:

- simulation loop는 여기서 미리 생성한 유효 element pool을 재사용한다.
- columns first, then girders 순서와 1-based ID 의미를 유지한다.

### `src/rust/src/sim_engine.rs`

역할:

- workfront별 후보 수집
- floor constraint / pattern buffer / stability 판정
- LocalStep 수집과 SimStep 병합
- scenario metrics 및 termination reason 계산

핵심 상태:

- `WorkfrontState`
  - `owned_ids`
  - `buffer_sequences`
  - `committed_floor`

- `SimConstraints`
  - `upper_floor_column_rate_threshold`
  - `lower_floor_completion_ratio_threshold`

- 버퍼/방출 관련 타입
  - `StepBufferDecision`
  - `StepCandidateMask`
  - `EmitResult`

핵심 구현 포인트:

- candidate collection 전에 `allowed_floors` 를 먼저 계산한다.
- 허용되지 않은 floor 는 수집 단계에서 early skip 한다.
- floor prefilter 는 single-candidate retry loop 앞단에서 강제된다.
- 후보는 하나씩 뽑아 현재 buffer 를 실제로 진전시키는지 검사한다.
- complete pattern 이더라도 stability fail 이면 즉시 infeasible rollback 한다.
- `committed_floor` 에서 후보가 끊기면 해당 buffer 만 rollback 한다.

### `src/rust/src/stability.rs`

역할:

- Dev/Sim 공통 패턴 판정 코어
- `classify_member_signature`
- `check_step_bundle_stability`

규칙:

- Step pass/fail 판단 기준은 여기의 공통 규칙을 따른다.
- 연결되지 않은 bundle 은 pass 할 수 없다.
- 단독 column 은 pass 할 수 없다.
- 단독 girder 는 이미 안정된 두 column 을 잇는 closure 일 때만 pass 할 수 있다.
- `Col + Girder` 는 인접 안정 구조와의 직교 연결까지 만족해야 pass 할 수 있다.
- Simulation 전용 오케스트레이션을 이 파일로 옮기지 않는다.

### `src/rust/src/graphics/ui.rs`

역할:

- Phase 3 공용 타입 정의
- `UiState` simulation 필드 관리
- `SimSequence`, `LocalStep`, `SimStep`, `SimScenario`, `TerminationReason` 정의

현재 중요한 타입 의미:

- `SimSequence`: 전역 sequence 번호를 가진 개별 설치 항목
- `LocalStep`: 한 workfront가 한 global step cycle 안에서 완성한 패턴
- `SimStep`: 여러 `LocalStep` 을 round-robin sequence로 병합한 global step

### `src/rust/src/graphics/sim_ui.rs`

역할:

- Settings / View / Result 패널 렌더링
- grid/workfront 선택 UI
- scenario table, playback, chart, export 버튼 렌더링
- trace logger 설정과 JSONL 저장 옵션 렌더링
- algorithm weights(`w1/w2/w3`) 설정 렌더링

### `src/rust/src/lib.rs`

역할:

- Simulation branch 진입점
- background simulation worker 실행
- progress, cancel, completed message polling
- sim grid/result를 app state에 반영
- 3D simulation view 인라인 렌더링

중요:

- 현재 시뮬레이션은 UI thread blocking 호출이 아니라 background worker 기반이다.
- 오래된 설명처럼 `run_all_scenarios()`를 UI update 루프에서 직접 막아 세우는 구조로 문서화하면 안 된다.

### Packaging / Build guardrail

- Python 패키지 공개 표면은 `import assyplan` 이고, Rust 확장 모듈은 `assyplan.assyplan_native` 서브모듈로 배치된다.
- Windows PDB 충돌 회피를 위해 Rust lib target 이름은 `assyplan_native`, 실행 바이너리 이름은 `assyplan`으로 분리 유지한다.
- editable 설치나 release wheel 검증은 반드시 프로젝트 루트에서 `python -m maturin develop`, `python -m maturin build --release` 형태로 실행한다.
- `--manifest-path src/rust/Cargo.toml`만 지정해 maturin을 실행하면 루트 `pyproject.toml`의 `python-source = "src/python"` 문맥이 빠져 `assyplan_native`만 설치되고 `import assyplan` 검증이 깨질 수 있다.

## 5. Step 생성의 canonical 흐름

현재 Step 생성은 workfront 즉시 방출 방식이 아니라 global step cycle 집계 방식이다.

1. 각 cycle에서 모든 workfront가 round 단위로 움직인다.
2. 각 workfront는 한 round에 최대 1개 element만 선택한다.
3. active workfront 수는 남은 미설치 부재 수에 비례해 줄어드는 방식으로 조정된다.
4. 초기 안정 구조가 비어 있으면 bootstrap bundle 을 먼저 선택한다.
5. 이후 증분 확장은 single-candidate retry loop 로 수행한다.
6. 선택 결과는 workfront 로컬 버퍼(`buffer_sequences`)에 쌓인다.
7. 버퍼 시그니처를 `classify_member_signature` 로 판정한다.
8. complete pattern + stability pass일 때만 `LocalStep` 을 만든다.
9. cycle 내에서 `LocalStep` 을 만든 workfront는 해당 cycle 남은 round에서 제외된다.
10. cycle 종료 시 여러 `LocalStep` 을 `SimStep::from_local_steps()` 로 병합한다.
11. 병합된 `SimStep.sequences` 는 round-robin collation이며 같은 round는 같은 sequence 번호를 공유한다.

문서/코드 검토 시 가장 먼저 볼 회귀 신호:

- Step이 workfront마다 즉시 방출되는가
- Sequence마다 기계적으로 Step이 하나씩 생기는가
- `local_steps` 정보가 사라졌는가
- Step 수가 Sequence 수와 거의 1:1로 무너졌는가
- 후보를 한 번에 여러 개 밀어 넣고 사후 subset 추출로 step 을 만드는가

## 6. Floor constraint canonical behavior

floor 선택은 단순 점수 경쟁이 아니라 제약 기반 타깃팅이다.

- 비잠금 상태: 허용된 floor 집합(`allowed_floors`)만 candidate collection 에 전달
- 잠금 상태: `committed_floor` 만 허용

upper/lower floor gate의 현재 의미:

- `upper_floor_threshold`
  - ratio gate 전용
  - top floor 또는 충분히 완료된 lower floor에서는 완화 가능

- `lower_floor_completion_ratio`
  - 상층 신규 진입 허용 조건
  - 기본값 `0.5`

추가 구현 포인트:

- `allowed_floors` 를 먼저 계산한 뒤 candidate collection에 전달한다.
- 허용되지 않은 floor를 수집 후반에 거르는 구조로 후퇴시키지 않는다.

## 7. Simulation UI / UX 구조

### Settings tab

- grid sliders
- constraint sliders
- trace logger + JSONL 옵션
- algorithm weights
- scenario count
- workfront 목록
- grid 변경 시 workfront/scenario reset

### View tab

- 2D x-y grid plan에서 workfront toggle
- simulation 결과가 있으면 3D construction/model view 사용
- orbit/pan/zoom은 기존 `ViewState` 재사용

### Result tab

- scenario table
- selected scenario details
- playback / seek / speed
- metrics summary
- charts
- export controls

## 8. Background execution / export

현재 Phase 3는 background task 흐름을 가진다.

- app state는 `sim_task_rx`, progress counters, cancel flag를 유지한다.
- 실행 중에는 status message와 progress가 갱신된다.
- cancel은 별도 flag로 worker에 전달된다.
- 완료 시 best scenario를 자동 선택하고 UI state를 갱신한다.
- debug export는 선택 시나리오 또는 전체 시나리오 대상으로 동작한다.

문서 작성/수정 시 금지:

- simulation이 항상 동기/blocking이라고 적는 것
- export 기능이 없다고 적는 것
- progress/cancel 구조를 누락하는 것
- 이미 제거된 forced-completion/throttle/rebase 상태를 현재 canonical flow 인 것처럼 적는 것

## 9. 자주 틀리는 포인트

### Sequence vs Step

- `SimSequence` 는 개별 설치 항목이다.
- `SimStep` 은 global step이다.
- `LocalStep` 은 그 중간 집계 단위다.

### 1-based vs 0-based

- scenario id, element id, step, sequence는 1-based다.
- `sim_selected_scenario` 만 vector index라서 0-based다.
- `grid_x`, `grid_y` 는 grid index라서 0-based다.

### Pattern field

- `SimStep.pattern` 은 human-readable summary다.
- multi-workfront 병합 step은 `Multi(n)` 형식이 될 수 있다.

### Algorithm weights

- 현재 코드에서 `w1`, `w2` 는 incremental single-candidate retry 점수에 사용된다.
- `w3` 는 bootstrap candidate bundle 점수에서 사용된다.
- 거리성은 incremental path 에서도 locality filter 와 `frontier_dist` 계산으로 구조적으로 반영된다.

### Simulation without CSV

- Simulation Mode는 CSV 데이터가 없어도 grid 기반으로 실행된다.
- `recalculate()` 의 no-data guard는 simulation 모드에서 우회된다.

### 3D view gotcha

- simulation 3D view는 `lib.rs` 쪽 인라인 렌더링이다.
- 같은 rect에 `allocate_rect` 를 두 번 호출하면 orbit 동작이 깨진다.
- construction mode ID 표시는 설치된 요소만 대상으로 해야 한다.

### Console build

- `main.rs` 의 console subsystem 설정은 release에서도 콘솔 로그를 유지하기 위한 것이다.
- `windows_subsystem = "windows"` 로 바꾸지 않는다.

## 10. 수정 체크리스트

Phase 3 관련 수정을 마친 뒤 최소 확인 항목:

1. `cargo build --release`
2. `Sequence != Step` 회귀 여부
3. 금지 패턴 step 승격 여부
4. floor lock/rollback/plan_exhausted 흐름 유지 여부
5. progress/cancel/export UI 동선 유지 여부
6. 1-based index 의미가 UI/metrics/export에서 유지되는지 확인

## 11. 빠른 진입점

- simulation 후보/제약/집계: `src/rust/src/sim_engine.rs`
- 공통 안정 판정: `src/rust/src/stability.rs`
- simulation 공용 타입/UI state: `src/rust/src/graphics/ui.rs`
- simulation 패널 UI: `src/rust/src/graphics/sim_ui.rs`
- simulation 실행/worker/app state: `src/rust/src/lib.rs`
- node 정렬 규칙 확인: `src/python/node_table.py`

## 12. 한 줄 요약

Phase 3의 정수는 grid 기반 element pool 위에서 multi-workfront sequence를 누적하고, complete pattern + stability pass일 때만 LocalStep/SimStep을 방출하며, floor gate와 background execution을 함께 유지하는 것이다.
