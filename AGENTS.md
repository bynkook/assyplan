# AssyPlan Development Guidelines

> **목적**: 이 파일은 AI 에이전트가 프로젝트를 처음 접할 때 파일과 폴더를 찾아 헤매는 시간을 줄이고, 개발 속도를 최대화하기 위한 핵심 참조 문서입니다. 코드를 탐색하기 전에 이 파일을 먼저 읽으세요.

---

## 1. 개발환경

- **OS**: Windows 11 x64
- **Shell**: cmd.exe (Git Bash 아님 — `SHELL=cmd.exe` 환경변수 영구 설정됨)
  - ⚠️ opencode는 `$SHELL` 환경변수를 우선 사용. Git Bash가 설치되어 있어도 cmd.exe로 강제됨.
  - bash 명령(if exist, del /F 등) 대신 PowerShell 또는 cmd.exe 명령 사용
  - 예: `powershell -Command "..."` 형태로 실행
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
├── devplandoc.md                          ← 핵심 스펙 문서 (설계 의도 파악 시 먼저 읽기)
│
├── .sisyphus/plans/
│   ├── dev-phase1.md                      ← Phase 1 계획서 (완료)
│   ├── dev-phase2.md                      ← Phase 2 계획서
│   └── dev-phase3.md                      ← Phase 3 계획서 (개요)
│
├── .opencode/
│   └── skills/
│       ├── dev-phase1/SKILLS.md           ← Phase 1 기술 스택 + 구현 패턴 참조
│       └── dev-phase2/SKILLS.md           ← Phase 2 기술 스택 + 구현 패턴 참조 ★현재
│
├── src/
│   ├── python/                            ← Python 레이어
│   │   ├── __init__.py
│   │   ├── data_loader.py                 ← CSV 로딩 + 인코딩 감지 (charset_normalizer)
│   │   ├── node_table.py                  ← 노드 추출 + ID 할당 (1부터 시작, x→y→z 정렬)
│   │   ├── element_table.py               ← 부재 분류 (Column/Girder)
│   │   ├── validators.py                  ← 구조 유효성 검사 스위트
│   │   ├── encoding.py                    ← 인코딩 유틸리티
│   │   └── data_io.py                     ← 직렬화/입출력
│   │
│   └── rust/                             ← Rust 레이어
│       ├── Cargo.toml                     ← Rust 의존성 (eframe 0.27, PyO3 0.21)
│       └── src/
│           ├── lib.rs                     ← PyO3 바인딩 + eframe app + recalculate()
│           ├── main.rs                    ← 단독 실행 진입점
│           ├── stability.rs               ← ★★ 핵심: 안정성 검증, 테이블 생성, 층 판별
│           └── graphics/
│               ├── mod.rs                 ← 모듈 내보내기
│               ├── renderer.rs            ← 2D 렌더링 엔진 (egui::Painter)
│               ├── step_renderer.rs       ← 스텝별 누적 렌더링 + 캐싱
│               ├── view_state.rs          ← 3D 뷰포트 상태 (카메라, 회전, 줌)
│               ├── axis_cube.rs           ← 방향 큐브 렌더링
│               └── ui.rs                  ← ★★ 핵심: UiState, 모든 탭 렌더링, Chart 1-3
│
├── tests/
│   └── python/
│       ├── test_data_loader.py
│       ├── test_node_table.py
│       ├── test_element_table.py
│       ├── test_validators.py
│       └── test_phase2.py                 ← Phase 2 E2E 통합 테스트
│
└── data/                                  ← 테스트용 CSV 입력 데이터
    └── (샘플 구조물 데이터 파일들)
```

---

## 3. Phase 상태

| Phase | 상태 | 설명 |
|-------|------|------|
| Phase 1 | ✅ 완료 | Development Mode - CSV 파싱, 유효성 검사, 3D 뷰어 |
| Phase 2 | 🔄 진행 중 | Step 기반 시공 시각화, Metric, Charts |
| Phase 3 | ⏳ 예정 | Simulation Mode - 실제 제약 적용, 시공 순서 자동 생성 |

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
- **부재 분류**: Column (x,y 동일, z 다름), Girder (x 또는 y 다름, z 동일)

### Phase 2 (Step Visualization + Metrics)
- **층 번호 판별** (`stability.rs::get_floor_level`):
  - 전체 노드의 unique z값을 정렬 → 1-indexed 위치가 층 번호
  - Column: i-node(하단) z 기준 (`get_column_floor`)
  - Girder: node_i z 기준 (수평 부재이므로 node_i == node_j z)
  - ⚠️ **성능 주의**: 호출마다 전체 노드 스캔 + unique z 정렬 반복. 현재 규모(수백~수천 부재)는 무방하나, Phase 3 이후 대형 구조물 대응 시 z-map 캐싱 최적화 필요
- **upper_floor_threshold**: 0.0~1.0 (기본값 0.3 = 30%)
  - Settings 탭 슬라이더로 조정
  - Phase 2에서는 **시각화(Chart 3)만** — 시공 순서 재계산은 Phase 3 예정
- **Simulation Mode**: 버튼만 표시, 비활성 상태 유지 (Phase 3까지)

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

### `src/rust/src/lib.rs`
- `recalculate()`: CSV 로드 → 테이블 생성 → UiState 업데이트 전체 파이프라인
- `update_floor_counts_for_step()`: 스텝별 층 데이터 업데이트
- PyO3 바인딩 (`PyNode`, `PyElement`, `PyRenderData`, `PyStepTable`)

---

## 6. 빌드 및 배포

### 빌드 명령
```powershell
# Rust 릴리즈 빌드
cd src\rust
cargo build --release

# 결과물을 루트로 복사
copy target\release\assyplan.exe ..\..\assyplan.exe
```

### Skills 문서 업데이트 규칙
1. 각 개발 단계 완료 또는 주요 변경 시 `.opencode/skills/dev-phase{N}/SKILLS.md` 업데이트
2. 기술 스택, 아키텍처, 구현 패턴, Gotcha 사항 문서화
3. **현재 활성**: `.opencode/skills/dev-phase2/SKILLS.md`

---

## 7. 개발 워크플로우

### 새 기능 추가 시 순서
1. `devplandoc.md` 에서 스펙 확인
2. `.opencode/skills/dev-phase2/SKILLS.md` 에서 관련 패턴 확인
3. `stability.rs` 또는 `ui.rs` 수정
4. `lsp_diagnostics` 로 오류 확인
5. `cargo build --release` 빌드
6. `target/release/assyplan.exe` → 루트 복사
7. SKILLS.md 업데이트

### 디버깅 순서
1. LSP diagnostics 확인 (`lsp_diagnostics` 도구)
2. `cargo build` 컴파일 오류 확인
3. 런타임: `stability.rs` 테스트 (`cargo test`)

---

## 8. 알려진 Gotcha 및 주의사항

### Windows 개발환경
- **Git Bash + `> nul` 리다이렉션**: Git Bash에서 `> nul`은 실제 `nul` 파일을 생성함. `SHELL=cmd.exe` 환경변수로 방지됨.
- **PowerShell 사용 권장**: bash 명령 대신 `powershell -Command "..."` 형태 사용
- **경로 구분자**: Windows는 `\`, Rust/POSIX는 `/` — Cargo.toml, 코드 내 경로는 `/` 사용

### Rust / egui
- **dashed line**: egui에 기본 제공 없음 → `while x < right { painter.line_segment(...); x += dash + gap; }` 패턴으로 직접 구현
- **floor_colors 스코프**: 여러 Chart가 공유하는 색상 배열은 클로저 밖 함수 스코프에 정의
- **step 인덱스**: `step_elements[s]`에서 s는 0-indexed이지만 UI/로직 상 step은 1-indexed. 접근 시 `step_elements[step - 1]` 또는 `step_elements.get(step)`으로 경계 체크 필수
- **upper_floor_threshold**: Phase 2에서는 시각화 전용. 실제 시공 순서 제약은 Phase 3에서 구현.

### Python
- **charset_normalizer 인코딩 이름**: `"utf_8"`, `"euc_kr"` (언더스코어) — 대시(`"utf-8"`) 아님
- **data_io.py LSP 오류**: Series → ConvertibleToInt 타입 오류 다수 존재. Phase 2 시작 전부터 있던 기존 이슈. 수정하지 말 것 (기능에 영향 없음).

---

## 9. 향후 대규모 모델 성능 향상 계획

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

## 10. Skills 문서 규칙

```
.opencode/skills/
├── dev-phase1/SKILLS.md    ← Phase 1 완료 (Python+Rust 기반 구조 + 렌더링)
├── dev-phase2/SKILLS.md    ← Phase 2 현재 (Step/Metric/Chart, 현재 활성)
└── dev-phase3/SKILLS.md    ← Phase 3 예정 시 생성
```

**작성 규칙**:
1. 단계 완료 또는 주요 기능 추가 시 해당 단계 SKILLS.md 즉시 업데이트
2. 포함 내용: 기술 스택, 모듈 아키텍처, 핵심 함수 시그니처, 데이터 흐름, Gotcha
3. 새 AI 에이전트가 SKILLS.md만 읽어도 해당 단계 코드를 이해하고 기여할 수 있는 수준으로 작성

---

## 11. Git 커밋 원칙

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
