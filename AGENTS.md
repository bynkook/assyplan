# AssyPlan Development Guidelines

> **목적**: 이 파일은 AI 에이전트가 프로젝트를 처음 접할 때 파일과 폴더를 찾아 헤매는 시간을 줄이고, 개발 속도를 최대화하기 위한 핵심 참조 문서입니다. 코드를 탐색하기 전에 이 파일을 먼저 읽으세요.

## Source of Truth

문서 우선순위는 아래 순서를 따른다.

1. `.github/copilot-instructions.md`
2. `AGENTS.md`
3. `devplandoc.md`
4. 실제 구현 코드

문서 간 충돌이 있으면 실제 코드로 확인한 뒤, 이 순서에 맞춰 문서를 갱신한다.

---

## 1. 개발환경

- **OS**: Windows 11 x64
- **Shell**: Git Bash (`SHELL=C:\Program Files\Git\bin\bash.exe` 환경변수 영구 설정됨)
  - ⚠️ opencode는 `$SHELL` 환경변수를 우선 사용. bash prefix(`export CI=true ...`)를 자동 주입하므로 반드시 bash가 실행되어야 함.
  - 이전에 `SHELL=cmd.exe`로 설정했다가 export prefix 오류 발생 → Git Bash로 변경 완료
  - bash 문법 사용 가능: `&&`, `export`, `$()` 등
- **Python**: 3.12+
- **Rust**: 2021 edition, eframe 0.27
- **가상환경**: `.venv/` (활성화: `.\.venv\Scripts\activate`)
- **의존성**: `requirements.txt`

---

## 2. 프로젝트 전체 구조 (파일 탐색 기준점)

```
C:\Users\BgKing\mycode\assyplan\          ← 프로젝트 루트
│
├── AGENTS.md                              ← 이 파일 (에이전트 가이드라인)
├── assyplan.exe                           ← 릴리즈 빌드 결과물 (루트에 복사본 유지)
├── requirements.txt                       ← Python 의존성
├── pyproject.toml                         ← Python 빌드 설정 (maturin)
├── devplandoc.md                          ← 핵심 스펙 문서 (설계 의도 + 용어/안정성 정본)
│
├── .sisyphus/plans/
│   ├── dev-phase1.md                      ← Phase 1 계획서 (완료)
│   ├── dev-phase2.md                      ← Phase 2 계획서
│   └── dev-phase3.md                      ← Phase 3 계획서 (개요)
│
├── .opencode/
│   └── skills/
│       ├── dev-phase1/SKILLS.md           ← Phase 1 기술 스택 + 구현 패턴 참조
│       ├── dev-phase2/SKILLS.md           ← Phase 2 기술 스택 + 구현 패턴 참조
│       └── dev-phase3/SKILLS.md           ← Phase 3 기술 스택 + 구현 패턴 참조 ★현재
│
├── src/
│   ├── python/                            ← Python 레이어
│   │   ├── __init__.py
│   │   ├── data_loader.py                 ← CSV 로딩 + 인코딩 감지 (charset_normalizer)
│   │   ├── node_table.py                  ← 노드 추출 + ID 할당 (1부터 시작, z→x→y 정렬)
│   │   ├── element_table.py               ← 부재 분류 (Column/Girder)
│   │   ├── validators.py                  ← 구조 유효성 검사 스위트
│   │   ├── encoding.py                    ← 인코딩 유틸리티
│   │   └── data_io.py                     ← 직렬화/입출력
│   │
│   └── rust/                             ← Rust 레이어
│       ├── Cargo.toml                     ← Rust 의존성 (eframe 0.27, PyO3 0.21, rayon 1)
│       └── src/
│           ├── lib.rs                     ← PyO3 바인딩 + eframe app + recalculate()
│           ├── main.rs                    ← 단독 실행 진입점 (cfg_attr console build)
│           ├── stability.rs               ← ★★ 핵심: 안정성 검증, 테이블 생성, 층 판별
│           ├── sim_grid.rs                ← ★★ Phase 3: 격자 기반 구조요소 풀 생성
│           ├── sim_engine.rs              ← ★★ Phase 3: Monte-Carlo 시뮬레이션 엔진 (rayon)
│           └── graphics/
│               ├── mod.rs                 ← 모듈 내보내기
│               ├── renderer.rs            ← 2D 렌더링 엔진 (egui::Painter)
│               ├── step_renderer.rs       ← 스텝별 누적 렌더링 + 캐싱
│               ├── view_state.rs          ← 3D 뷰포트 상태 (카메라, 회전, 줌)
│               ├── axis_cube.rs           ← 방향 큐브 렌더링
│               ├── ui.rs                  ← ★★ 핵심: UiState, 모든 탭 렌더링, Chart 1-3
│               └── sim_ui.rs              ← ★★ Phase 3: Simulation 탭 UI (Settings/View/Result)
│
├── tests/
│   └── python/
│       ├── test_data_loader.py
│       ├── test_node_table.py
│       ├── test_element_table.py
│       ├── test_validators.py
│       └── test_phase2.py                 ← Phase 2 E2E 통합 테스트
├── src/rust/tests/
│   └── regression_compare.rs              ← Dev/Sim fingerprint 회귀 테스트
│
└── data/                                  ← 테스트용 CSV 입력 데이터
    └── (샘플 구조물 데이터 파일들)
```

---

## 3. Phase 상태

| Phase | 상태 | 설명 |
|-------|------|------|
| Phase 1 | ✅ 완료 | Development Mode - CSV 파싱, 유효성 검사, 3D 뷰어 |
| Phase 2 | ✅ 완료 | Step 기반 시공 시각화, Metric, Charts |
| Phase 3 | 🔄 진행 중 | Simulation Mode - Monte-Carlo 시공 순서 자동 생성, 패턴 기반 Step/Sequence |

---

## 4. 핵심 기술 결정사항

### 공통 규칙 (절대 변경 금지)
- **ID는 1부터 시작**: Node ID, Element ID, Step, Sequence — 0은 절대 사용하지 않음
- **외부 차트 crate 불가**: 모든 차트는 `egui::Painter`로 직접 구현
- **egui 버전**: 0.27.2 고정 (eframe 0.27)
- **Python 버전**: 3.12+

### Phase 1 (Development Mode)
- **Python**: pandas 2.0+, numpy 1.26+, charset_normalizer 3.0+
- **Rust**: eframe 0.27 (egui + wgpu), PyO3 0.21
- **인코딩 감지**: charset_normalizer (`utf_8`, `euc_kr` — 언더스코어 형식)
- **Node ID 할당**: 모든 unique 좌표 수집 → (x, y, z) 오름차순 정렬 → 1부터 순번
  - 현재 canonical 구현은 `(z, x, y)` 오름차순이다.
- **부재 분류**: Column (x,y 동일, z 다름), Girder (x 또는 y 다름, z 동일)

### Phase 2 (Step Visualization + Metrics)
- **층 번호 판별** (`stability.rs::get_floor_level`):
  - 전체 노드의 unique z값을 정렬 → 1-indexed 위치가 층 번호
  - Column: i-node(하단) z 기준 (`get_column_floor`)
  - Girder: node_i z 기준 (수평 부재이므로 node_i == node_j z)
  - ⚠️ **성능 주의**: 호출마다 전체 노드 스캔 + unique z 정렬 반복. 현재 규모(수백~수천 부재)는 무방하나, Phase 3 이후 대형 구조물 대응 시 z-map 캐싱 최적화 필요
- **upper_floor_threshold**: 0.0~1.0 (기본값 0.3 = 30%)
  - Settings 탭 슬라이더로 조정
  - Phase 2에서는 **시각화(Chart 3)만** — 실제 시공 순서 제약은 Phase 3에서 구현

### Phase 3 (Simulation Mode)

#### ⚠️ 핵심 용어 정의 (Sequence vs Step) — 반드시 숙지

| 용어 | 정의 | 설명 |
|------|------|------|
| **Sequence** | **단위 시간 (Time Unit)** | 각 Sequence에서 N개 workfront가 각각 1개씩 부재를 **동시에** 설치. Sequence 1, 2, 3... = 시간 흐름. **N workfront = N개 부재/Sequence** |
| **Step** | **구조 안정 조건을 만족하는 시공 완료 부재 그룹** | 패턴 기반 (Col, ColCol, ColGirder 등). 여러 Sequence의 결과물이 모여 하나의 안정적인 Step을 구성 |

**예시** (workfront 2개 = 작업조 2조):
```
Sequence 1: WF-A → Col1, WF-B → Col2  (동시 2개 설치)
Sequence 2: WF-A → Col3, WF-B → Girder1  (동시 2개 설치)
Sequence 3: WF-A → Girder2, WF-B → Col4  (동시 2개 설치)
...
마지막 Sequence: 남은 부재 1개 → 1개만 설치
종료: 모든 부재 설치 완료 → 0개
```

#### 기술 구현
- **sim_grid.rs**: 격자 설정(nx, ny, nz, dx, dy, dz)으로 전체 node/element pool 자동 생성
- **sim_engine.rs**: Monte-Carlo + Weighted Sampling, rayon 병렬 시나리오 생성
  - `SimSequence`: 단위 시간에 설치되는 부재 기록 (1-indexed global sequence_number)
  - `LocalStep`: 한 workfront 가 global step cycle 안에서 완성한 로컬 패턴 단위
  - `SimStep`: 여러 `LocalStep` 을 병합한 global step (`local_steps`, `pattern` 필드 포함)
  - 허용 패턴: `Col`, `Girder`, `ColCol`, `ColGirder`, `GirderGirder`, `ColColGirder`, `ColGirderCol`, `ColGirderGirder`, `ColColGirderGirder`, `ColColGirderColGirder`
  - **금지 패턴**: `Col→Col→Col`, `Col→Col→Col→Girder` (3개 연속 Column 금지)
- **floor target/lock canonical**:
  - floor 선택은 층 간 점수경쟁이 아니라 제약 기반으로 결정한다.
  - 비잠금 상태에서는 제약을 통과한 floor를 대상으로 타깃 floor를 고른다.
  - 잠금 상태(`committed_floor`)에서는 해당 floor 후보만 선택한다.
  - 잠금 floor 실패 시 즉시 rollback 하고 `last_failed_floor`로 동일 floor 즉시 재시도 루프를 회피한다.
  - `planned_pattern` 소진 + step 미완성(`plan_exhausted`) 시 재계획을 강제한다.
- **candidate floor prefilter**:
  - workfront round마다 `allowed_floors` 를 먼저 계산한다.
  - 허용되지 않은 floor 는 candidate collection 단계에서 즉시 제외한다.
  - legacy/optimized candidate collection parity 를 유지해야 한다.
- **upper_floor_threshold**: `sim_engine.rs`에서 `threshold * 100.0`으로 변환 후 `stability.rs`에 전달
- **simulation execution**:
  - `lib.rs` 는 simulation을 background worker 로 실행한다.
  - progress / cancel / export 흐름을 현재 canonical behavior로 본다.
- **console build**: `main.rs` `#![cfg_attr(not(debug_assertions), windows_subsystem = "console")]`

#### Simulation 기본값 (현재)
- Grid Y lines (`GridConfig.ny`): `8`
- Lower-floor completion ratio threshold (`lower_floor_completion_ratio`): `0.8`
- Lower-floor forced completion threshold (`lower_floor_forced_completion`): `10`
- Scenario count (`sim_scenario_count`): `2`

---

## 5. 주요 파일별 역할 요약

### `src/rust/src/stability.rs` ← 가장 복잡한 파일
함수 목록 (LSP symbols로 빠르게 확인 가능):
- `get_floor_level(node_id, nodes)` → i32: 노드의 층 번호 (1-indexed)
- `get_column_floor(element, nodes)` → i32: Column의 층 (i-node 기준)
- `get_floor_column_counts(elements, nodes)` → HashMap<i32, usize>: 층별 기둥 수
- `check_floor_installation_constraint(target_floor, installed_ids, all_elements, nodes, threshold)` → (bool, f64)
- `build_step_elements_map(...)` → Vec<Vec<(i32, String, i32)>>: 스텝별 (element_id, type, floor)
- `generate_all_tables(nodes, elements)` → TableGenerationResult
- `get_floor_column_data(...)` → Vec<(i32, usize, usize)>: (floor, total, installed)

현재 역할 주의:
- `stability.rs` 는 Development Mode 테이블 생성의 본체다.
- 동시에 Dev/Sim 이 함께 쓰는 공통 패턴 판정 코어를 가진다.
- 공통 규칙 예: `classify_member_signature`, `check_step_bundle_stability`

### `src/rust/src/graphics/ui.rs` ← UI 전체 담당
주요 공개 함수:
- `render(ui_state, ctx)`: 전체 레이아웃 렌더링
- `render_header(ui, state)`: 상단 버튼 바
- `render_settings_tab(ui, state)`: 설정 탭 (threshold 슬라이더 포함)
- `render_result_tab(ui, state)` → `render_result_tab_inner`: Chart 1, 2, 3
- `render_view_tab(ui, state)`: 3D 뷰포트

**Chart 위치** (`render_result_tab_inner` 내부):
1. Chart 1: Element Type Distribution (누적 Column/Girder 수)
2. Chart 2: Floor-by-Floor Installation Progress (층별 기둥 설치 수)
3. Chart 3: Upper-Floor Column Installation Rate (상층부 기둥 설치율) — 공식: (N+1층 누적 설치수) / (N층 누적 설치수), 분모=0이면 0.0, 레전드: F{N+1}/F{N}

**중요 패턴**: `floor_colors` 배열은 `render_result_tab_inner` 함수 스코프에 정의됨 (Chart 2/3 공유). 절대 chart 클로저 안에 넣지 말 것.

**Phase 3 신규 타입** (ui.rs):
- `SimSequence { element_id: i32, sequence_number: usize }` — 단위 시간에 설치되는 부재 (1-indexed globally)
- `LocalStep { workfront_id, element_ids, floor, pattern }` — workfront 로컬 완성 패턴
- `SimStep { workfront_id, element_ids, sequences: Vec<SimSequence>, floor, pattern: String, local_steps: Vec<LocalStep> }` — 구조 안정 조건을 만족하는 global step
- `SimStep::from_local_steps(local_steps, start_seq)` — round-robin collation 기반 병합 헬퍼

### `src/rust/src/lib.rs`
- `recalculate()`: CSV 로드 → 테이블 생성 → UiState 업데이트 전체 파이프라인
- `run_simulation()`: 입력 검증 → background worker 생성 → grid/scenario 계산 → best scenario 선택
- `poll_simulation_task()`: progress, cancel, completion/failure 상태 반영
- `export_simulation_debug()`: 선택 시나리오 또는 전체 시나리오 debug CSV/summary export
- `update_floor_counts_for_step()`: 스텝별 층 데이터 업데이트
- PyO3 바인딩 (`PyNode`, `PyElement`, `PyRenderData`, `PyStepTable`)
- Sim 3D view (View 탭 인라인): 설치된 부재 렌더링 + 시나리오 ComboBox + step nav bar

### `src/rust/src/sim_grid.rs` (Phase 3 신규)
격자 설정(nx, ny, nz, dx, dy, dz)으로 전체 node/element pool 자동 생성.
- `SimGrid::new(nx, ny, nz, dx, dy, dz)` → SimGrid
- IDs 1-indexed: nodes는 `(z→x→y)` 정렬 의미를 유지하고, elements는 columns first then girders 순서를 유지한다.

### `src/rust/src/sim_engine.rs` (Phase 3 신규)
Monte-Carlo + Weighted Sampling, rayon 병렬 시나리오 생성.
- `run_scenario(scenario_id, grid, workfronts, seed, weights, threshold)` → SimScenario
- `run_all_scenarios_with_progress_and_cancel(count, grid, workfronts, weights, constraints, progress, cancel)` → Vec<SimScenario>
- `try_build_pattern(seed_id, grid, installed_ids, ...)` — 패턴 확장 (금지 패턴 차단)
- threshold 변환: `threshold * 100.0` (stability.rs는 0~100 범위 기대)
- global step cycle 집계, floor prefilter, rollback/replan 로직의 중심 파일이다.

현재 역할 주의:
- `sim_engine.rs` 는 시뮬레이션 전용 오케스트레이션 파일이다.
- 후보 생성, weighted sampling, global step cycle 집계는 여기서 수행한다.
- 적합 안정 패턴의 공통 판정 코어는 `stability.rs` 를 재사용한다.

### `src/rust/src/graphics/sim_ui.rs` (Phase 3 신규)
Simulation 탭 전용 UI (Settings, View, Result).
- `render_sim_settings(ui, state)` → bool
- `render_sim_view(ui, state)` → bool
- `render_sim_result(ui, state)`
- `render_scenario_comparison_chart(ui, state)` (private, Result 하단 다중 시나리오 비교)

---

## 6. 빌드 및 배포

### 빌드 명령
```bash
# Rust 릴리즈 빌드 (workdir: src/rust)
cargo build --release

# 결과물을 루트로 복사 (Git Bash — copy 아님, cp 사용)
cp target/release/assyplan.exe ../../assyplan.exe

# Python editable install / mixed project validation (run at repo root)
python -m maturin develop

# Release wheel build (run at repo root)
python -m maturin build --release
```

- Windows PDB 충돌 회피를 위해 Rust lib target은 `assyplan_native`, Python 확장 모듈 경로는 `assyplan.assyplan_native`, 실행 바이너리 이름은 `assyplan`으로 분리 유지한다.
- `maturin` 명령은 루트 `pyproject.toml`의 `python-source = "src/python"` 문맥을 타야 하므로 repo root에서 실행한다. `--manifest-path src/rust/Cargo.toml`만 쓰면 `import assyplan` 검증이 깨질 수 있다.

### Skills 문서 업데이트 규칙
1. 각 개발 단계 완료 또는 주요 변경 시 `.opencode/skills/dev-phase{N}/SKILLS.md` 업데이트
2. 기술 스택, 아키텍처, 구현 패턴, Gotcha 사항 문서화
3. **현재 활성**: `.opencode/skills/dev-phase3/SKILLS.md`

---

## 7. 개발 워크플로우

### 새 기능 추가 시 순서
1. `devplandoc.md` 에서 스펙 확인
2. `.opencode/skills/dev-phase3/SKILLS.md` 에서 관련 패턴 확인
3. Development Mode/공통 판정 규칙이면 `stability.rs`, Simulation orchestration 이면 `sim_engine.rs`, UI면 `ui.rs` 또는 `sim_ui.rs` 수정
4. `get_errors` 또는 빌드 결과로 오류 확인
5. `cargo build --release` 빌드
6. `target/release/assyplan.exe` → 루트 복사
7. SKILLS.md 업데이트

### 오류 수정 원칙
- 증상 패치는 금지한다. 로그, 스크린샷, 재현 결과는 현상을 좁히는 용도로만 사용하고, 수정은 반드시 구조적 원인 확인 뒤 진행한다.
- `sim_engine.rs` 같은 상태기계 성격의 코드는 먼저 책임 경계를 분리해서 본다. 특히 throttle, plan refresh, buffer reset, emit eligibility 를 섞어서 한 번에 땜질하지 않는다.
- 새 `if` 분기나 예외 처리 추가 전, 기존 상태 전이에서 무엇이 잘못 연결됐는지 설명 가능해야 한다. 설명이 안 되면 패치를 미룬다.

### 디버깅 순서
1. `get_errors` 또는 컴파일 오류 확인
2. `cargo build` 컴파일 오류 확인
3. Rust 단위 테스트 확인 (`cargo test --lib`)
4. Dev/Sim 회귀 확인이 필요하면 `cargo test --test regression_compare`

### 테스트 기준 메모
- `tests/python/` 는 현재 `data.csv` 를 샘플 입력으로 사용한다. 과거 `data.txt` 기준 설명은 stale 이다.
- Python 출력 리포트 테스트의 기준 파일명은 `dev_validation_report.txt`, `dev_stability_report.txt`, `dev_metrics_summary.txt`, `dev_step_statistics.txt` 다.
- Rust 회귀 기준은 `src/rust/tests/regression_compare.rs` 의 Dev/Sim fingerprint 값이다. canonical Sim/Dev 동작을 의도적으로 바꾼 경우에만 상수를 갱신한다.

---

## 8-A. 문서 기준점

- 문서 우선순위의 최상위는 `.github/copilot-instructions.md` 이다.
- 안정성/패턴/용어의 현재 정본은 `devplandoc.md` 내부 `용어 및 Step 적합 안정 조건 정본` 절이다.
- 기존 `STABILITY_ANALYSIS.md` 는 `devplandoc.md` 로 통합되어 삭제되었다.
- 작업 전 문서 기준이 필요하면 먼저 `.github/copilot-instructions.md`, 그 다음 `devplandoc.md`, 그 다음 이 문서를 본다.

---

## 9. 알려진 Gotcha 및 주의사항

### Windows 개발환경
- **Git Bash + `> nul` 리다이렉션**: Git Bash에서 `> nul`은 실제 `nul` 파일을 생성함. `SHELL=cmd.exe` 환경변수로 방지됨.
- **PowerShell 사용 권장**: bash 명령 대신 `powershell -Command "..."` 형태 사용
- **경로 구분자**: Windows는 `\`, Rust/POSIX는 `/` — Cargo.toml, 코드 내 경로는 `/` 사용

### Rust / egui
- **dashed line**: egui에 기본 제공 없음 → `while x < right { painter.line_segment(...); x += dash + gap; }` 패턴으로 직접 구현
- **floor_colors 스코프**: 여러 Chart가 공유하는 색상 배열은 클로저 밖 함수 스코프에 정의
- **step 인덱스**: `step_elements[s]`에서 s는 0-indexed이지만 UI/로직 상 step은 1-indexed. 접근 시 `step_elements[step - 1]` 또는 `step_elements.get(step)`으로 경계 체크 필수
- **upper_floor_threshold**: Phase 2에서는 시각화 전용. 실제 시공 순서 제약은 Phase 3에서 구현.
- **axis_cube 면 렌더링**: `Shape::convex_polygon` 사용 금지. egui의 feathering normal 계산 버그(issue #1226)로 특정 orbit 각도에서 face가 비정상적으로 크게 렌더링됨. 반드시 `Mesh::add_triangle`로 직접 삼각형 분할하여 그릴 것.
- **axis_cube clip_rect**: `ui.painter_at(viewport).with_clip_rect(small_rect)`는 `small_rect.intersect(viewport)`로 작동하여 실제 격리가 안 됨. `ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("...")))` 로 독립 레이어 painter를 생성한 뒤 `with_clip_rect` 적용해야 정확히 동작.
- **view_state scroll zoom**: `handle_input()`에서 scroll 감지 조건으로 `response.hovered()` 사용 금지. Construction mode에서 TopBottomPanel(nav bar)이 같은 프레임에 존재할 경우 `hovered()`가 false를 반환하여 scroll event가 무시됨. 반드시 `ui.input(|i| i.pointer.hover_pos()).map(|p| response.rect.contains(p))` 로 직접 비교할 것.
- **Sequence 모드 transform**: Construction Sequence 모드에서 렌더링 직전 반드시 `step_data.base.calculate_transform(rect, &view_state)` 호출 필요. `render_data.calculate_transform()`만 호출하면 `step_render_data.base`의 transform이 갱신되지 않아 zoom/pan이 반영되지 않음.
- **grid bubble trimming 일관성**: `renderer.rs`(Model view)와 `step_renderer.rs`(Construction view)는 grid line 렌더링 코드가 각각 독립 구현됨. 한쪽을 수정하면 다른 쪽도 반드시 동일하게 수정할 것. bubble 원 내부 침범 방지: `let p1_trimmed = p1 + (p2-p1).normalized() * bubble_radius` 후 `line_segment([p1_trimmed, p2], ...)`.
- **console build** (`main.rs`): `#![cfg_attr(not(debug_assertions), windows_subsystem = "console")]` — release 빌드에서도 콘솔 출력 허용. `"windows"`로 변경하면 콘솔이 완전히 숨겨짐 (금지).
- **sim 3D view orbit — single allocate_rect**: lib.rs View 탭 인라인 sim 3D view에서 `allocate_rect`는 반드시 한 번만 호출. 두 번 호출하면 orbit 이중 처리 버그 발생. hover 체크는 `ui.input(|i| i.pointer.hover_pos()).map(|p| rect.contains(p))`로 직접 비교.
- **construction mode element ID 필터링**: Sequence/Step 모드에서 node/element ID는 **설치된 부재에만** 표시. 전체 elements 루프 금지 — 설치된 ID 목록만 순회할 것.
- **simulation worker flow**: simulation은 현재 background worker + progress/cancel 구조다. 예전처럼 UI thread에서 직접 `run_all_scenarios()` 를 블로킹 호출하는 설명이나 회귀를 만들지 말 것.
- **simulation requires workfront**: 현재 구현은 default workfront를 자동 생성하지 않는다. workfront가 비어 있으면 simulation은 에러 메시지와 함께 시작되지 않는다.
### Python
- **charset_normalizer 인코딩 이름**: `"utf_8"`, `"euc_kr"` (언더스코어) — 대시(`"utf-8"`) 아님
- **data_io.py LSP 오류**: Series → ConvertibleToInt 타입 오류 다수 존재. Phase 2 시작 전부터 있던 기존 이슈. 수정하지 말 것 (기능에 영향 없음).

---

## 10. 향후 대규모 모델 성능 향상 계획

Phase 3 이후 구조물 규모가 커질 때를 대비한 최적화 포인트:

### 9.1 `get_floor_level()` z-map 캐싱
**현재**: 호출마다 전체 노드 스캔 + unique z 정렬 → O(N) per call, O(N×M) total
```rust
// 현재 구현 (stability.rs)
pub fn get_floor_level(node_id: i32, nodes: &[StabilityNode]) -> i32 {
    let mut unique_z: Vec<i64> = nodes.iter()...collect::<HashSet<_>>()...sorted();
    // 매번 반복
}
```
**목표**: z-map을 한 번만 생성하여 모든 floor_level 조회에 재사용
```rust
// 개선 방향
fn build_z_level_map(nodes: &[StabilityNode]) -> HashMap<i64, i32> { ... }
// 이후 get_floor_level(node_id, nodes, z_map: &HashMap<i64,i32>) 로 시그니처 변경
```
- 예상 효과: 10,000 부재 모델에서 테이블 생성 시간 ~70% 단축

### 9.2 누적 렌더링 최적화
**현재**: `step_renderer.rs`의 `get_cumulative_elements(step)` — 처음 호출 시 O(step × avg_elements) 계산 후 캐싱
**목표**: 대형 모델에서 초기 캐시 워밍업 비용이 커질 경우, 백그라운드 스레드에서 미리 계산

### 9.3 egui Painter 클리핑
**현재**: 모든 부재를 매 프레임 paint
**목표**: 뷰포트 밖 부재 skip (frustum culling 등)
- egui의 `painter.clip_rect()`를 활용한 CPU-side 클리핑

### 9.4 대형 모델 정의
- "대형 구조물" 기준: 부재 수 > 5,000개 또는 층 수 > 30층
- 현재 타깃: 수백~수천 부재 규모 → 현재 구현으로 충분

---

## 11. Skills 문서 규칙

```
.opencode/skills/
├── dev-phase1/SKILLS.md    ← Phase 1 완료 (Python+Rust 기반 구조 + 렌더링)
├── dev-phase2/SKILLS.md    ← Phase 2 완료 (Step/Metric/Chart)
└── dev-phase3/SKILLS.md    ← Phase 3 현재 활성 (Simulation Mode, Monte-Carlo, SimStep/SimSequence)
```

**작성 규칙**:
1. 단계 완료 또는 주요 기능 추가 시 해당 단계 SKILLS.md 즉시 업데이트
2. 포함 내용: 기술 스택, 모듈 아키텍처, 핵심 함수 시그니처, 데이터 흐름, Gotcha
3. 새 AI 에이전트가 SKILLS.md만 읽어도 해당 단계 코드를 이해하고 기여할 수 있는 수준으로 작성

---

## 12. Git 커밋 원칙

### 커밋 워크플로우 (반드시 준수)

1. **`git status` 먼저 확인**: 변경된 파일 전체 목록을 먼저 파악한다.
2. **제외 파일은 `.gitignore` 처리**: 커밋 불필요한 파일/폴더(빌드 아티팩트, 임시 파일 등)는 `.gitignore`에 추가한다.
3. **나머지 전체 커밋**: `git add -A` 또는 `git add .`로 남은 변경사항 전체를 커밋한다.

### 금지 사항

- ❌ `git status` 확인 없이 커밋하지 않는다.
- ❌ 임의로 파일을 골라 일부만 커밋하지 않는다 (`git add file1 file2` 방식으로 선별 금지).
- ❌ 커밋에서 제외할 파일을 단순히 스테이징에서 빼는 방식으로 처리하지 않는다 — 반드시 `.gitignore`에 추가한다.

### .gitignore 추가 대상 기준

- 빌드 결과물: `target/`, `*.exe`, `*.pyd`, `*.dll`
- Python 캐시: `__pycache__/`, `.venv/`
- maturin 빌드 산출물: `src/python/assyplan/`, `src/graphics/`
- 출력/생성 파일: `output/`, `*.csv` (단, 테스트 입력 데이터는 예외)
- OS/임시 파일: `nul`, `UsersBgKingAppDataLocalTempegui-*/`, `*.tmp`
