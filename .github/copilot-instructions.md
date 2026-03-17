# AssyPlan Copilot Instructions

이 문서는 AssyPlan 프로젝트에서 다음 코드 수정 작업을 수행할 때 참조하는 압축형 정본 가이드다.

## 1) Source of Truth

우선순위는 아래 순서로 판단한다.

1. `.github/copilot-instructions.md`
2. `AGENTS.md`
3. `devplandoc.md`
4. `STABILITY_ANALYSIS.md`
5. 실제 구현 코드

문서 간 충돌이 있으면 실제 코드로 검증하고, 이후 이 문서를 기준으로 맞춘다.

검증된 현재 사실:

- Node ID 부여 순서는 `z -> x -> y` 오름차순이며 1부터 시작한다.
- `Sequence` 와 `Step` 은 다른 개념이다.
- `Step` 은 개별 부재 단위가 아니라 패턴 기반 안정 단위다.

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
- `stability.rs`: 안정성 검사와 sequence/step 테이블 생성 핵심
- `sim_grid.rs`: 시뮬레이션용 전체 node/element pool 생성
- `sim_engine.rs`: Monte-Carlo 기반 시나리오 생성, pattern 선택, step/sequence 분리
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

### Workfront

- 각 workfront 는 수직적으로 여러 층을 가질 수 있으나, 한 sequence 에서는 한 층에서만 작업한다.
- 모든 workfront 는 동시에 시작하며 전역 우선순위는 두지 않는다.
- workfront 확장은 기둥에서 시작하며 거더 단독 시작은 허용하지 않는다.

## 6) Stability Rules For Step Generation

Step 적합 안정 조건은 개별 부재가 아니라 패턴 단위로 판단한다.

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
- 어떤 층의 미설치 부재가 5개 이하이면 그 층을 우선 마감하는 방향을 유지한다.

## 8) Known Current Risk

`sim_engine.rs` 의 핵심 위험은 Step 생성이 패턴 기반이 아니라 개별 요소 단위로 흘러갈 수 있다는 점이다.

수정 시 반드시 지켜야 할 점:

- workfront 별로 sequence 를 버퍼링한 뒤 패턴 완성 여부를 검사한다.
- 완성 패턴 + 안정 조건 PASS 일 때만 Step 을 방출한다.
- Sub pattern 단계에서는 Step 을 생성하지 않는다.
- Step 생성 로직을 바꿀 때 UI, metrics, scenario summary 가 모두 여전히 `Sequence != Step` 가정을 유지하는지 함께 확인한다.

## 9) Editing Rules

- 근본 원인을 고친다. 예외 무시, 임시 하드코딩, 의미 없는 방어 로직 추가를 피한다.
- Python 은 입출력과 구조 데이터 준비를 담당하고, Rust 는 계산/시뮬레이션/UI 핵심을 담당한다. 책임을 섞지 않는다.
- PyO3 경계를 바꾸면 Python 호출부와 Rust 바인딩을 함께 점검한다.
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

## 12) Short Summary For Future Tasks

AssyPlan 수정 작업의 핵심은 아래 한 줄로 요약된다.

Python 이 구조 데이터를 준비하고, Rust 가 안정 조건과 시뮬레이션을 계산하며, 모든 변경은 `1-based ID`, `Sequence/Step 분리`, `패턴 기반 안정 판정`, `증분 확장 우선` 원칙을 깨지 않아야 한다.