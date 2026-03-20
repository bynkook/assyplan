# AssyPlan Copilot Instructions

이 문서는 AssyPlan 프로젝트에서 다음 코드 수정 작업을 수행할 때 참조하는 압축형 정본 가이드다.

## 1) Source of Truth

우선순위는 아래 순서로 판단한다.

1. `.github/copilot-instructions.md`
2. `AGENTS.md`
3. `devplandoc.md`
4. 실제 구현 코드

문서 간 충돌이 있으면 실제 코드로 검증하고, 이후 이 문서를 기준으로 맞춘다.

검증된 현재 사실:

- Node ID 부여 순서는 `z -> x -> y` 오름차순이며 1부터 시작한다.
- `Sequence` 와 `Step` 은 다른 개념이다.
- `Step` 은 개별 부재 단위가 아니라 패턴 기반 안정 단위다.
- 안정성/패턴/용어의 정본 문서는 `devplandoc.md` 내부 `용어 및 Step 적합 안정 조건 정본` 절이다.

## 2) Project Goal

AssyPlan 은 철골 구조물의 3D 형상 또는 그리드 정의를 바탕으로 다음 두 가지를 수행한다.

- Development Mode: 사용자 CSV 를 읽고 node/element/sequence/step/metric 데이터를 만들고 3D 로 검토한다.
- Simulation Mode: 그리드와 workfront 설정을 바탕으로 시공 가능한 sequence 와 stability-aware step 을 자동 생성한다.

핵심 목적은 단순 렌더링이 아니라, 적합 및 안정 조건을 만족하는 시공 순서를 계산하고 시각화하는 것이다.

## 3) Architecture Overview

### Python 레이어

- 위치: `src/python/`
- 역할: CSV 입력, 인코딩 처리, 노드/부재/시퀀스/스텝 테이블 생성, 검증, 출력 저장
- 사용자 친화적인 입출력과 전처리를 담당한다.

주요 파일:

- `data_loader.py`: CSV 로드 및 인코딩 처리
- `node_table.py`: unique node 추출, `z -> x -> y` 정렬, 1-based ID 부여
- `element_table.py`: Column/Girder 분류
- `validators.py`, `stability_validators.py`: 기본 검증 로직
- `precedent_graph.py`, `sequence_table.py`, `step_table.py`, `workfront.py`: 개발 모드 시공 순서 관련 데이터 구성
- `data_io.py`, `output_manager.py`, `metrics.py`: 저장 및 메트릭 출력

### Rust 레이어

- 위치: `src/rust/src/`
- 역할: 성능이 중요한 안정성 판정, 테이블 계산, 시뮬레이션 엔진, egui 기반 UI 렌더링
- PyO3 바인딩을 통해 Python 과 연결된다.

주요 파일:

- `lib.rs`: PyO3 바인딩, 전체 재계산 파이프라인, 시뮬레이션 진입점
- `stability.rs`: Development Mode 테이블 생성 + Dev/Sim 공통 패턴 판정 코어
- `sim_grid.rs`: 시뮬레이션용 전체 node/element pool 생성
- `sim_engine.rs`: Monte-Carlo 기반 시나리오 생성, pattern 선택, global step cycle 집계
- `main.rs`: 단독 실행 진입점

### Rust UI 하위 모듈

- 위치: `src/rust/src/graphics/`
- `ui.rs`: 전체 UI 상태와 탭 렌더링
- `sim_ui.rs`: Simulation Mode UI
- `renderer.rs`: 일반 모델 렌더링
- `step_renderer.rs`: step 누적 렌더링 및 캐시
- `view_state.rs`, `axis_cube.rs`: 뷰 조작 및 축 큐브

### 테스트

- 위치: `tests/python/`
- Python 레이어와 개발 모드 시퀀스/스텝 관련 검증이 중심이다.

## 4) Current Phase Understanding

- Phase 1: CSV 입력, node/element 생성, 기본 검증, 3D 개발 모드
- Phase 2: construction sequence table, workfront step table, metrics, step 기반 시각화
- Phase 3: grid 기반 Simulation Mode, multi-workfront, Monte-Carlo, weighted sampling, stability-aware step 생성

현재 수정 작업은 대체로 Phase 3 정합성과 Phase 2/3 경계의 일관성을 건드릴 가능성이 높다.

## 5) Non-Negotiable Domain Rules

### IDs and ordering

- 모든 ID 는 1부터 시작한다. 0은 사용하지 않는다.
- Node ID 는 현재 구현 기준 `z -> x -> y` 정렬 후 부여한다.
- Step, Sequence, Workfront 번호도 1-based 를 유지한다.

### Member model

- 부재 타입은 기본적으로 `Column` 또는 `Girder` 다.
- Column: `(x, y)` 동일, `z` 만 달라지는 수직 부재
- Girder: 같은 층에서 `x` 또는 `y` 한 축만 변하는 수평 부재
- 대각선 부재는 허용하지 않는다.
- zero-length 부재, 중복 부재, orphan 노드, 중복 ID 는 오류다.
- `z = 0` 레벨 거더는 허용하지 않는다.

### Sequence vs Step

- `Sequence`: 개별 부재 설치 순서 또는 단위 시간 개념
- `Step`: 안정 조건을 만족하는 패턴 기반 부재 그룹
- Step 수는 Sequence 수보다 훨씬 적어야 정상이다.
- 개별 부재 하나를 기계적으로 Step 으로 승격하면 안 된다.
- 같은 round 동시 설치는 동일 sequence 번호를 공유한다.

### Workfront

- 각 workfront 는 수직적으로 여러 층을 가질 수 있으나, 한 sequence 에서는 한 층에서만 작업한다.
- 모든 workfront 는 동시에 시작하며 전역 우선순위는 두지 않는다.
- workfront 확장은 기둥에서 시작하며 거더 단독 시작은 허용하지 않는다.

## 6) Stability Rules For Step Generation

Step 적합 안정 조건은 개별 부재가 아니라 패턴 단위로 판단한다.

최소 요약:
- 공통 패턴 판정 규칙은 `stability.rs` 에 있고, Sim 은 이를 재사용한다.
- Dev 계산 오케스트레이션은 `stability.rs`, Sim 오케스트레이션은 `sim_engine.rs` 가 담당한다.
- 국소 안정 문맥이 없을 때는 독립 bootstrap 규칙으로만 pass 가능하다.

### Bootstrap

- 독립 구조의 최소 단위는 기둥 3개 + 거더 2개다.
- 거더 2개는 기둥 상단에서 90도 직교해야 한다.
- 최초 독립 step 또는 bootstrap 은 이 최소 조립 조건을 만족해야 한다.

### Incremental extension

- 기존 안정 구조에 인접하게 붙는 증분 패턴이 우선이다.
- 새 Column 은 바닥 또는 이미 안정 판정된 하부 Column 상단에 지지되어야 한다.
- 새 Girder 는 양단이 안정 구조에 의해 지지되어야 한다.
- 캔틸레버 상태가 되면 안 된다.

### Allowed / disallowed patterns

허용 또는 완성 가능 패턴:

- `Girder` 단독 폐합
- `ColGirder`
- `ColGirderGirder` 단, 캔틸레버 금지
- `ColColGirderGirder`
- `ColGirderColGirder`
- `ColColGirderColGirder` 또는 Bootstrap

Sub pattern 으로만 취급해야 하는 경우:

- `ColCol`
- `GirderGirder`
- `ColColGirder`
- `ColGirderCol`

절대 금지 패턴:

- `Col -> Col -> Col`
- `Col -> Col -> Col -> Girder`

## 7) Simulation Strategy Guardrails

Phase 3 시뮬레이션 엔진은 다음 원칙을 따라야 한다.

- Monte-Carlo 샘플링은 유지하되, 후보 생성은 minimal incremental attachment 를 우선한다.
- 큰 독립 단위보다 기존 구조에 바로 붙는 작은 증분 후보를 우선한다.
- 우선 후보 순서는 대체로 `1기둥+1거더`, `1기둥+2거더`, `2기둥+1거더`, `2기둥+2거더`, 마지막으로 독립 `3기둥+2거더` 다.
- 기존 구조에 연결 가능한 증분 후보가 하나라도 있으면 독립 bootstrap 후보보다 우선한다.
- 상층부 기둥 설치율 제약 threshold 기본값은 `0.3` 이다.
- 하층부 기둥 완료율 제약 threshold 기본값은 `0.5` 이다.

### Current canonical behavior (2026-03-20)

- 시뮬레이션 엔진의 Step 생성은 workfront 독립 방출이 아니라 **global step cycle 집계 방식**을 따른다.
- 한 global step cycle 내부에서 각 workfront 는 sequence round 마다 최대 1개 부재를 선택한다.
- 각 round 의 active workfront 수는 남은 미설치 부재 수에 비례해 줄어드는 비율 제어 방식으로 정한다.
- workfront 로컬 버퍼가 완성 패턴 + 안정 조건 PASS 에 도달하면 `LocalStep` 으로 cycle 수집 버퍼에 저장한다.
- 같은 cycle 에서 local step 생성에 성공한 workfront 는 해당 cycle 의 남은 라운드에서 제외한다.
- cycle 종료 시 수집된 여러 `LocalStep` 을 1개의 `SimStep` 으로 병합한다 (`local_steps: Vec<LocalStep>` 보존).
- global step 의 `element_ids` 는 모든 local step element union 이다.
- 각 workfront 의 sequence 는 승인된 local step 의 `element_ids` 를 생성 순서대로 이어붙인 **WF 연속 이력** 이다.
- Sequence 뷰는 cycle(step) 순서 기반으로 정렬한다. 각 cycle 블록 내에서 WF별 원소를 position interleave하여 전역 sequence 를 구성하고 누적으로 표시한다. 이 방식은 지지 기둥이 반드시 거더보다 먼저 표시됨을 보장한다.
- Step 뷰는 안정성 패턴 단위 뷰이며, Sequence 뷰와 의미를 섞지 않는다.
- 따라서 `Sequence != Step` 가정은 유지되며, multi-workfront 상황에서 Step 수는 Sequence 수보다 작아야 정상이다.
- bootstrap 은 `stable_ids` 와 `cycle_local_steps` 가 모두 비어 있을 때만 시작하며, workfront anchor 근처 bootstrap bundle 을 weighted sampling 으로 고른다.
- bootstrap 이후 증분 확장은 **single-candidate retry loop** 로 동작한다. 각 workfront 는 후보를 하나씩 뽑아 보고, 현재 버퍼를 진전시키지 못하는 후보는 버리고 다음 후보를 다시 시도한다.
- candidate 수집 전에 round 단위 `allowed_floors` 를 계산하고, 허용되지 않은 floor 는 candidate collection 단계에서 즉시 제외한다.
- floor 선택은 별도 점수 경쟁이 아니라 `allowed_floors + committed_floor` 제약으로 처리한다.
	- 비잠금 상태: `upper_floor_column_rate_threshold`, `lower_floor_completion_ratio_threshold` 를 통과한 floor 집합만 허용한다.
	- 잠금 상태: `committed_floor` 고정 (해당 floor 후보만 선택)
- 상층 기둥 비율 제약(`upper_floor_column_rate_threshold`)은 **ratio gate 전용**으로 적용한다.
	- 최상층(`max_floor`)은 ratio gate를 면제한다. (B)
	- 하층 기둥 완료율이 `lower_floor_completion_ratio_threshold` 이상이면 ratio gate를 면제한다. (C)
- buffer 분류는 `StepBufferDecision::Incomplete(mask)`, `Complete(pattern)`, `Invalid` 로만 다룬다. 증분 후보는 `StepCandidateMask` 를 만족해야만 버퍼를 계속 확장할 수 있다.
- complete pattern 이더라도 `check_step_bundle_stability` 에서 실패하면 즉시 infeasible rollback 한다.
- 잠금 floor에서 더 이상 유효 후보가 없거나 버퍼가 invalid/infeasible 이 되면 즉시 rollback 한다.
	- rollback 은 현재 버퍼 부재만 workfront local ownership 에서 제거하고 `buffer_sequences`, `committed_floor` 를 초기화하는 것으로 끝낸다.
- 현재 엔진에는 `forced completion`, `planned_pattern`, `last_failed_floor`, `active throttling`, `floor/spatial rebase` 같은 보정 레이어가 없다.
- simulation trace 는 text 로그를 기본으로 하고, 필요하면 JSONL 을 추가 저장할 수 있다.
- trace 에는 workfront 별 승인된 local step 이력(`sim.wf.approved_local_step_recorded`)이 포함된다.
- 시뮬레이션 실행은 UI 스레드 블로킹 호출이 아니라 background worker + progress/cancel 흐름으로 유지한다.
- Simulation 기본 UI 값은 `GridConfig.ny = 8`, `lower_floor_completion_ratio = 0.5`, `sim_scenario_count = 2` 이다.
- 시뮬레이션 결과는 선택 시나리오 또는 전체 시나리오 기준 debug export CSV/summary 경로를 유지한다.

## 8) Known Current Risk

`sim_engine.rs` 의 핵심 위험은 Step 생성의 집계 규칙이 깨져 `Sequence` 와 `Step` 이 다시 1:1에 가까워지는 회귀다.

수정 시 반드시 지켜야 할 점:

- workfront 별로 sequence 를 버퍼링한 뒤 패턴 완성 여부를 검사한다.
- 완성 패턴 + 안정 조건 PASS 일 때만 `LocalStep` 을 생성한다.
- global step cycle 끝에서만 `LocalStep` 들을 병합하여 최종 `SimStep` 을 방출한다.
- Sub pattern 단계에서는 Step 을 생성하지 않는다.
- single-candidate retry loop 를 우회하여 후보 묶음을 한 번에 뿌린 뒤 나중에 유효 subset 만 추출하는 구조로 되돌리지 않는다.
- `StepCandidateMask` 와 `check_step_bundle_stability` 의 역할을 섞지 않는다. 전자는 버퍼 확장 가능 타입 필터, 후자는 complete pattern 의 적합 안정 판정이다.
- `WorkfrontState` 는 현재 `owned_ids`, `buffer_sequences`, `committed_floor` 중심의 단순 상태를 유지한다. 예전 보정 상태 필드를 되살리는 방향의 문서화나 구현을 피한다.
- sequence 번호는 1-based + global 연속성을 유지하고, 동일 round 동시 설치는 동일 sequence 번호를 공유해야 한다.
- floor prefilter 를 약화시켜 허용되지 않은 floor 후보를 뒤늦게 거르는 구조로 되돌리지 않는다.
- background simulation task/progress/cancel/export 흐름을 깨는 동기식 회귀를 만들지 않는다.
- Step 생성 로직을 바꿀 때 UI, metrics, scenario summary 가 모두 여전히 `Sequence != Step` 가정을 유지하는지 함께 확인한다.

## 9) Editing Rules

- 근본 원인을 고친다. 예외 무시, 임시 하드코딩, 의미 없는 방어 로직 추가를 피한다.
- 증상 패치는 금지한다. 로그나 재현 결과는 증상을 좁히는 근거로만 쓰고, 수정 전 반드시 상태 전이와 책임 경계를 기준으로 구조적 원인을 먼저 검토한다.
- 시뮬레이션 모드 빌드의 기능 디버깅에서는 로그를 확인하며 작업한다. 이때 증상 패치와 휴리스틱 패치는 금지하고, 반복 루프/탈출 구조와 부재적합안정조건 로직의 실제 오판정 경로를 먼저 추적한다.
- 새로운 조건문이나 예외 분기를 추가하기 전에, 해당 문제가 throttle, plan refresh, buffer reset, emit eligibility 중 어느 책임에서 발생했는지 먼저 분리해서 확인한다.
- Python 은 입출력과 구조 데이터 준비를 담당하고, Rust 는 계산/시뮬레이션/UI 핵심을 담당한다. 책임을 섞지 않는다.
- PyO3 경계를 바꾸면 Python 호출부와 Rust 바인딩을 함께 점검한다.
- AssyPlan 패키징은 `assyplan` 패키지 + `assyplan.assyplan_native` 확장 서브모듈 구조를 유지한다. Windows PDB 충돌 회피를 위해 Rust lib target 이름은 `assyplan_native`, 실행 바이너리 이름은 `assyplan`으로 분리한다.
- `maturin develop`/`maturin build` 검증은 반드시 프로젝트 루트에서 실행한다. `--manifest-path src/rust/Cargo.toml`만으로 실행하면 루트 `pyproject.toml`의 `python-source = "src/python"` 문맥이 빠져 `import assyplan` 검증이 왜곡될 수 있다.
- 성능 관련 수정은 step 누적 렌더링과 floor 계산 경로를 의식한다.
- 기존 공개 데이터 구조 이름과 1-based 의미를 함부로 바꾸지 않는다.
- 차트는 외부 crate 대신 현재 egui painter 방식과 기존 스타일을 유지한다.

## 10) Practical Workflow

수정 전 확인 순서:

1. 이 문서에서 해당 기능의 규칙을 확인한다.
2. 변경 대상이 Python 인지 Rust 인지 먼저 구분한다.
3. 관련 파일을 좁힌다.

기능별 진입점:

- CSV 로드/전처리: `src/python/data_loader.py`, `src/python/encoding.py`
- node/element 규칙: `src/python/node_table.py`, `src/python/element_table.py`, `src/python/validators.py`
- 개발 모드 sequence/step: `src/python/sequence_table.py`, `src/python/step_table.py`, `src/rust/src/stability.rs`
- 시뮬레이션 후보/패턴/종료 조건: `src/rust/src/sim_engine.rs`
- 그리드 생성: `src/rust/src/sim_grid.rs`
- UI 및 차트: `src/rust/src/graphics/ui.rs`, `src/rust/src/graphics/sim_ui.rs`

## 11) Verification Baseline

코드 수정 후 최소한 아래를 확인한다.

- Python 변경: 관련 `tests/python/` 테스트 또는 영향 범위 수동 확인
- Rust 변경: `cargo build --release` 기준 컴파일 확인
- Step/Sequence 변경: step 수가 sequence 수와 거의 1:1 로 무너지는 회귀가 없는지 확인
- 안정성 변경: 금지 패턴이 step 으로 인정되지 않는지 확인
- UI 변경: step 이동, scenario 전환, metric 표시가 1-based 인덱스를 유지하는지 확인
- Simulation UI/engine 변경: progress 표시, cancel, export 동선이 유지되는지 확인

## 12) Short Summary For Future Tasks

AssyPlan 수정 작업의 핵심은 아래 한 줄로 요약된다.

Python 이 구조 데이터를 준비하고, Rust 가 안정 조건과 시뮬레이션을 계산하며, 모든 변경은 `1-based ID`, `Sequence/Step 분리`, `패턴 기반 안정 판정`, `증분 확장 우선` 원칙을 깨지 않아야 한다.