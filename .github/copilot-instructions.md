# AssyPlan Copilot Instructions

## Source of truth

문서 간 충돌이 있으면 아래 우선순위로 판단한다.

1. `.github/copilot-instructions.md`
2. `AGENTS.md`
3. `devplandoc.md`
4. 현재 구현 코드

문서 내용이 실제 동작과 다르면 코드로 검증한 뒤 이 문서를 최신 기준으로 맞춘다.

## 빌드 및 테스트 명령

별도 언급이 없으면 저장소 루트에서 실행한다.

### Python 테스트

```powershell
python -m pytest tests\python -q
python -m pytest tests\python\test_workfront.py -q
python -m pytest tests\python\test_phase2.py -k test_e2e_pipeline_with_sample_data -q
```

Python 테스트는 실제 `data.csv` 샘플을 사용하며, `tests\python\test_phase2.py`가 Development Mode 파이프라인의 대표 통합 테스트다.

### Rust 빌드 및 회귀 확인

```powershell
cargo build --manifest-path src\rust\Cargo.toml --release
cargo test --manifest-path src\rust\Cargo.toml --test regression_compare
cargo test --manifest-path src\rust\Cargo.toml --test regression_compare regression_guard_sim_fingerprint_v3 -- --exact
```

`src\rust\tests\regression_compare.rs`는 Dev/Sim fingerprint 회귀 가드다. Development Mode 또는 Simulation Mode의 canonical 동작을 의도적으로 바꿨다면 구현과 함께 fingerprint 상수도 같이 갱신한다.

### Python/Rust 패키지 검증

```powershell
python -m maturin develop
python -m maturin build --release
```

`maturin`은 반드시 저장소 루트에서 실행한다. 루트 `pyproject.toml`의 `python-source = "src/python"` 및 `assyplan.assyplan_native` 모듈 경로 문맥이 필요하므로, `src/rust/Cargo.toml`만 직접 지정하는 방식으로 대체하지 않는다.

## 상위 아키텍처

AssyPlan은 단순 3D 렌더러가 아니라, 구조 데이터를 바탕으로 시공 순서와 안정성 기반 Step을 계산하고 시각화하는 Python/Rust 혼합 애플리케이션이다.

- `src/python/`은 CSV 로드, 인코딩 처리, node/element 테이블 생성, precedent graph 구성, workfront 식별, sequence/step 준비, 출력 저장을 담당한다.
- `src/rust/src/`는 공통 안정성 판정, Development Mode 테이블 계산, Simulation grid 생성, Monte Carlo 기반 시나리오 생성, egui UI 렌더링을 담당한다.
- `src/rust/src/lib.rs`는 Python-Rust 연결의 중심이다. PyO3 바인딩, 재계산 파이프라인, background simulation 실행, progress/cancel polling, simulation sequence row 구성이 여기 모인다.
- `src/rust/src/stability.rs`는 공통 도메인 코어다. Development Mode 테이블 생성이 여기 있고, Simulation Mode도 패턴 분류와 안정성 체크를 이 파일에서 재사용한다.
- `src/rust/src/sim_grid.rs`는 시뮬레이션에 필요한 전체 node/element pool을 한 번에 생성하고 검증해, 엔진이 이미 유효한 구조 집합에서만 후보를 고르도록 만든다.
- `src/rust/src/sim_engine.rs`는 시뮬레이션 오케스트레이션 파일이다. bootstrap 선택, incremental candidate retry, floor gating, local-step 완성, rollback, cycle 단위 step 병합이 여기 있다.
- `src/rust/src/graphics/ui.rs`와 `src/rust/src/graphics/sim_ui.rs`는 Development/Simulation UI를 렌더링한다. 특히 `ui.rs`는 `LocalStep`, `SimStep`, `SimScenario`, `SimWorkfront` 같은 핵심 simulation 타입도 정의한다.

큰 구조는 다음처럼 이해하면 된다. Python은 입력과 구조 데이터 준비를 맡고, Rust는 안정성 판단과 시뮬레이션 계산, 데스크톱 UI를 맡는다.

## 핵심 규칙과 관례

### ID와 정렬 규칙

- 외부에 노출되는 ID는 모두 1부터 시작한다. node, element, step, sequence, workfront에 0-based 번호를 도입하지 않는다.
- node 정렬의 canonical 기준은 `z -> x -> y` 오름차순이다. `src/python/node_table.py`와 `src/rust/src/sim_grid.rs`가 이 규칙을 공유한다.
- simulation grid에서는 element ID가 결정적이어야 하며, 항상 column을 먼저 배정하고 그 다음 girder를 배정한다.

### Sequence와 Step은 다른 개념이다

- `Sequence`는 설치 순서/시간 단위다.
- `Step`은 안정 조건을 만족한 패턴 단위다.
- 둘이 다시 1:1 관계로 무너지면 안 된다. multi-workfront 시뮬레이션에서는 하나의 global step 안에 여러 workfront의 local completion이 합쳐질 수 있다.

`lib.rs`의 simulation sequence 재구성과 UI/metric 해석은 이 분리를 전제로 한다. Step 생성 로직을 수정할 때는 `Sequence != Step` 가정이 계속 유지되는지 같이 확인한다.

### 안정성 로직은 패턴 기반이다

- 부재 타입은 `Column`과 `Girder`만 사용한다.
- Column은 `(x, y)`가 같고 `z`만 달라지는 수직 부재다.
- Girder는 한 층 내에서 평면 한 축만 변하는 수평 부재다.
- 대각 부재, zero-length 부재, 중복 부재, orphan node, 중복 ID, `z = 0`의 girder는 모두 invalid다.

패턴 분류의 정본은 `stability.rs`의 `classify_member_signature`, `StepBufferDecision`, `check_step_bundle_stability`다. 시뮬레이션 전용 예외 규칙을 다른 파일에 복제하지 말고 공통 규칙을 이 계층에서 확장한다.

### 시뮬레이션 엔진 가드레일

- bootstrap은 초기 독립 안정 구조를 만들 때만 사용하고, 그 이후에는 새로운 독립 묶음보다 기존 안정 구조에 붙는 incremental attachment를 우선한다.
- 각 workfront는 하나의 global step cycle 안에서 round마다 최대 1개 부재만 선택한다.
- workfront buffer가 complete pattern에 도달하고 안정성까지 통과했을 때만 `LocalStep`을 방출한다.
- cycle 종료 시 여러 local step을 `SimStep::from_local_steps`로 병합해 하나의 global step으로 만든다.
- `ColCol`, `GirderGirder`, `ColColGirder`, `ColGirderCol` 같은 sub-pattern은 중간 buffer 상태일 뿐, 방출 가능한 step이 아니다.
- locked floor에서 invalid 또는 infeasible buffer가 발생하면 억지 완성이 아니라 local ownership/buffer 초기화 rollback이 canonical 동작이다.

`sim_engine.rs`를 수정할 때는 single-candidate retry loop와 candidate collection 이전 floor prefilter 흐름을 유지한다.

### 패키징 규칙

- Rust 라이브러리 타깃 이름은 `assyplan_native`를 유지한다.
- Python 확장 모듈 경로는 `assyplan.assyplan_native`를 유지한다.
- 실행 파일 이름은 `assyplan`을 유지한다.

이 분리는 Windows 환경에서 패키징 및 디버그 산출물 충돌을 피하기 위한 규칙이다.

### UI 규칙

- 차트는 `egui::Painter`로 직접 그린다. 외부 chart crate로 바꾸지 않는다.
- simulation 실행은 Rust 앱 상태에 연결된 background task 흐름을 유지해야 한다. progress, cancel, export 동선을 깨는 동기식 회귀를 만들지 않는다.
