# 개발 1단계 최종 계획서

## TL;DR

> **목표**: 철골 구조물 3D 시각화 시스템 **개발 모드(Development Mode)** 구현
> 
> **산출물**:
> - Python 데이터 파서 (CSV → Node/Element 테이블)
> - 데이터 검증 모듈 (orphan, 중복, 축 평행 등)
> - Rust 3D 그래픽 뷰어 (eframe 기반)
> - PyO3 통합 (Python ↔ Rust 데이터 전달)
> - UI 버튼 (recalc, reset, mode toggle)
> 
> **예상 기간**: 2일
> **병렬 실행**: YES - 5 Waves
> **Critical Path**: 스캐폴드 → 데이터 파서 → eframe 앱 → PyO3 바인딩 → 통합

---

## Context

### 원본 요청
`devplandoc.md`의 개발 1단계 요구사항 구현

### 기술 스택 (수정안 - 빠른 구현)

| 항목 | 선택 | 이유 |
|------|------|------|
| **Python** | 3.12 + pandas + numpy | 표준, 문서화됨 |
| **Rust 그래픽** | eframe (egui + wgpu 통합) | 설정 최소화, Windows DX12 자동 처리 |
| **Python-Rust 바인딩** | PyO3 + maturin | 표준, 성숙함 |
| **인코딩** | charset_normalizer | 표준, 빠름 |
| **테스트** | pytest + cargo test | 표준 |

### 핵심 설계 결정

1. **eframe 사용**: wgpu + egui-wgpu + winit 수동 설정 대신 통합 프레임워크 사용
2. **데이터 정규화**: Python에서 모든 데이터 가공 후 Rust에 전달
3. **단방향 데이터 흐름**: Python → Rust (복잡한 양방향 동기화 회피)

### 샘플 데이터 분석

`data.txt` 기준:
- 기둥(Column): 105개 (CF###)
- 거더(Girder): 154개 (GF###)
- Grid 범위: X(0~43200), Y(0~48000), Z(0~70000)
- 층고: 10000 단위

---

## Work Objectives

### Core Objective
사용자 CSV 업로드 → 데이터 검증 → 3D 그래픽 렌더링 파이프라인 구축

### Concrete Deliverables

1. `src/python/` - Python 데이터 처리 모듈
2. `src/rust/` - Rust 그래픽 엔진
3. `pyproject.toml` - 프로젝트 설정
4. `tests/` - 테스트 스위트
5. `SKILLS.md` - 개발 가이드

### Definition of Done

- [ ] CSV 파일 로드 성공 (UTF-8/EUC-KR)
- [ ] Node/Element 테이블 생성 (ID 정렬: x→y→z, 1부터 시작)
- [ ] 모든 검증 규칙 통과 시에만 렌더링
- [ ] 3D 뷰: X-Y/Y-Z/Z-X 평면 + Orbit/Zoom/Pan
- [ ] recalc/reset 버튼 동작
- [ ] pytest + cargo test 통과

### Must Have

- CSV 파싱 (UTF-8 + EUC-KR)
- Node/Element 테이블 생성
- 검증 로직 (orphan, 중복 ID, zero-length, 축 평행)
- 3D 뷰어 (grid, node dot, element line)
- ID 라벨 on/off
- recalc/reset 버튼

### Must NOT Have (Guardrails)

- Simulation Mode 구현 (버튼만 표시, 비활성)
- 대각선 부재 허용
- ID 0 사용
- 검증 없이 렌더링

---

## Verification Strategy

### Test Decision

- **Infrastructure exists**: NO (신규 생성)
- **Automated tests**: YES (TDD)
- **Framework**: pytest (Python) + cargo test (Rust)

### QA Policy

모든 태스크는 Agent-Executed QA Scenarios 포함:
- **Frontend/UI**: Playwright (playwright skill)
- **CLI/Backend**: Bash (curl, python -c)
- **Library/Module**: Bash (python REPL)

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately - 스캐폴드):
├── Task 1: Python 프로젝트 스캐폴드 [quick]
├── Task 2: Rust 프로젝트 스캐폴드 [quick]
└── Task 3: pyproject.toml + maturin 설정 [quick]

Wave 2 (After Wave 1 - 코어 모듈):
├── Task 4: CSV 파서 (UTF-8/EUC-KR) [quick]
├── Task 5: eframe 최소 앱 [quick]
└── Task 6: 인코딩 감지 유틸리티 [quick]

Wave 3 (After Wave 2 - 데이터 처리):
├── Task 7: Node 테이블 생성 (x→y→z 정렬) [quick]
├── Task 8: Element 테이블 생성 (Column/Girder 분류) [quick]
└── Task 9: 기본 렌더링 (grid, node, element) [quick]

Wave 4 (After Wave 3 - 검증 + 통합):
├── Task 10: 검증 로직 구현 [quick]
├── Task 11: PyO3 바인딩 [quick]
└── Task 12: UI 레이아웃 + 버튼 [quick]

Wave 5 (After Wave 4 - 최종):
├── Task 13: 통합 테스트 [quick]
├── Task 14: SKILLS.md 작성 [writing]
└── Task 15: 최종 검증 [quick]

Critical Path: 1 → 4 → 7 → 10 → 11 → 13
Parallel Speedup: ~50% faster than sequential
Max Concurrent: 3
```

### Dependency Matrix

- **1-3**: — — 4-6
- **4-6**: 1-3 — 7-9
- **7-9**: 4-6 — 10-12
- **10-12**: 7-9 — 13-15
- **13-15**: 10-12 — FINAL

### Agent Dispatch Summary

- **Wave 1**: 3 × `quick`
- **Wave 2**: 3 × `quick`
- **Wave 3**: 3 × `quick`
- **Wave 4**: 3 × `quick`
- **Wave 5**: 2 × `quick` + 1 × `writing`

---

## TODOs

- [x] 1. Python 프로젝트 스캐폴드 생성

  **What to do**:
  - `src/python/` 디렉토리 생성
  - `__init__.py`, `data_loader.py`, `node_table.py`, `element_table.py`, `validators.py`, `data_io.py` 파일 생성
  - `tests/python/` 디렉토리 생성

  **Must NOT do**:
  - 구현 코드 작성 (스캐폴드만)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2, 3)
  - **Blocks**: Tasks 4-6
  - **Blocked By**: None

  **References**:
  - `devplandoc.md:88-93` - 입력데이터 컬럼명 정의

  **Acceptance Criteria**:
  - [ ] `src/python/` 디렉토리 존재
  - [ ] 6개 Python 파일 존재 (비어있어도 OK)
  - [ ] `tests/python/` 디렉토리 존재

  **QA Scenarios**:
  ```
  Scenario: Python 스캐폴드 확인
    Tool: Bash
    Steps:
      1. ls src/python/
      2. ls tests/python/
    Expected Result: 모든 파일/디렉토리 존재
    Evidence: .sisyphus/evidence/task-01-scaffold.txt
  ```

  **Commit**: NO (Wave 1 완료 후 일괄)

- [x] 2. Rust 프로젝트 스캐폴드 생성

  **What to do**:
  - `src/rust/` 디렉토리 생성
  - `cargo init --lib` 실행
  - `Cargo.toml`에 eframe, pyo3 의존성 추가
  - `src/lib.rs`, `src/graphics/mod.rs`, `src/graphics/renderer.rs` 파일 생성

  **Must NOT do**:
  - 구현 코드 작성 (스캐폴드만)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3)
  - **Blocks**: Tasks 5, 11
  - **Blocked By**: None

  **References**:
  - eframe docs: https://docs.rs/eframe/

  **Acceptance Criteria**:
  - [ ] `src/rust/Cargo.toml` 존재
  - [ ] `cargo check` 성공 (컴파일 에러 없음)
  - [ ] eframe, pyo3 의존성 포함

  **QA Scenarios**:
  ```
  Scenario: Rust 스캐폴드 확인
    Tool: Bash
    Steps:
      1. cd src/rust && cargo check
    Expected Result: "Finished dev [unoptimized]" 메시지
    Evidence: .sisyphus/evidence/task-02-cargo-check.txt
  ```

  **Commit**: NO (Wave 1 완료 후 일괄)

- [x] 3. pyproject.toml + maturin 설정

  **What to do**:
  - 프로젝트 루트에 `pyproject.toml` 생성
  - Python 3.12, pandas, numpy, pytest 의존성 정의
  - maturin 빌드 설정

  **Must NOT do**:
  - 불필요한 의존성 추가

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2)
  - **Blocks**: Tasks 4-6
  - **Blocked By**: None

  **References**:
  - maturin docs: https://www.maturin.rs/

  **Acceptance Criteria**:
  - [ ] `pyproject.toml` 존재
  - [ ] `requires-python = ">=3.12"` 설정
  - [ ] pandas, numpy, pytest 의존성 포함
  - [ ] maturin build-backend 설정

  **QA Scenarios**:
  ```
  Scenario: pyproject.toml 확인
    Tool: Bash
    Steps:
      1. cat pyproject.toml | grep "requires-python"
      2. cat pyproject.toml | grep "pandas"
    Expected Result: 모든 설정 존재
    Evidence: .sisyphus/evidence/task-03-pyproject.txt
  ```

  **Commit**: YES (Wave 1 완료 후)
  - Message: `chore: initial project scaffold`
  - Files: `pyproject.toml`, `src/python/`, `src/rust/`

- [x] 4. CSV 파서 구현 (UTF-8/EUC-KR)

  **What to do**:
  - `data_loader.py`에 CSV 파싱 함수 구현
  - charset_normalizer로 인코딩 자동 감지
  - 부재_ID, node_i_x/y/z, node_j_x/y/z, 선행부재_ID 컬럼 읽기
  - 데이터프레임 반환

  **Must NOT do**:
  - 하드코딩된 인코딩 사용
  - 컬럼명 검증 생략

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`python-code-style`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 5, 6)
  - **Blocks**: Tasks 7-8
  - **Blocked By**: Tasks 1-3

  **References**:
  - `devplandoc.md:88-93` - 입력데이터 컬럼명
  - `data.txt` - 샘플 데이터

  **Acceptance Criteria**:
  - [ ] UTF-8 CSV 로드 성공
  - [ ] EUC-KR CSV 로드 성공
  - [ ] 필수 컬럼 누락 시 예외 발생
  - [ ] pytest 테스트 통과

  **QA Scenarios**:
  ```
  Scenario: UTF-8 CSV 로드
    Tool: Bash
    Steps:
      1. python -c "from data_loader import load_csv; df = load_csv('data.txt'); print(len(df))"
    Expected Result: 259 (샘플 데이터 행 수)
    Evidence: .sisyphus/evidence/task-04-utf8-load.txt

  Scenario: 컬럼 누락 예외
    Tool: Bash
    Steps:
      1. python -c "from data_loader import load_csv; load_csv('invalid.csv')"
    Expected Result: ValueError 발생
    Evidence: .sisyphus/evidence/task-04-missing-column.txt
  ```

  **Commit**: NO (Wave 2 완료 후 일괄)

- [x] 5. eframe 최소 앱 구현

  **What to do**:
  - `src/rust/src/lib.rs`에 eframe 앱 구조체 정의
  - 기본 윈도우 생성
  - 빈 캔버스 렌더링
  - `cargo run`으로 실행 확인

  **Must NOT do**:
  - 복잡한 UI 구현 (최소 앱만)
  - 데이터 렌더링 (이후 태스크)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 4, 6)
  - **Blocks**: Task 9
  - **Blocked By**: Tasks 1-3

  **References**:
  - eframe template: https://github.com/emilk/eframe_template

  **Acceptance Criteria**:
  - [ ] `cargo run` 실행 시 윈도우 표시
  - [ ] 윈도우 닫기 정상 동작
  - [ ] cargo test 통과

  **QA Scenarios**:
  ```
  Scenario: eframe 앱 실행
    Tool: Bash
    Steps:
      1. cd src/rust && cargo run --release
      2. 3초 후 프로세스 종료
    Expected Result: 윈도우 표시됨 (스크린샷 불가하므로 exit code 0으로 확인)
    Evidence: .sisyphus/evidence/task-05-eframe-run.txt
  ```

  **Commit**: NO (Wave 2 완료 후 일괄)

- [x] 6. 인코딩 감지 유틸리티 구현

  **What to do**:
  - `encoding.py`에 인코딩 감지 함수 구현
  - charset_normalizer 사용
  - UTF-8 우선, EUC-KR 폴백

  **Must NOT do**:
  - chardet 사용 (charset_normalizer가 더 빠름)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`python-code-style`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 4, 5)
  - **Blocks**: Task 4
  - **Blocked By**: Tasks 1-3

  **References**:
  - charset_normalizer docs: https://pypi.org/project/charset-normalizer/

  **Acceptance Criteria**:
  - [ ] UTF-8 파일 감지 성공
  - [ ] EUC-KR 파일 감지 성공
  - [ ] pytest 테스트 통과

  **QA Scenarios**:
  ```
  Scenario: 인코딩 감지
    Tool: Bash
    Steps:
      1. python -c "from encoding import detect_encoding; print(detect_encoding('data.txt'))"
    Expected Result: "utf-8" 또는 "EUC-KR"
    Evidence: .sisyphus/evidence/task-06-encoding.txt
  ```

  **Commit**: YES (Wave 2 완료 후)
  - Message: `feat: add data parser and graphics core`
  - Files: `src/python/data_loader.py`, `src/python/encoding.py`, `src/rust/src/lib.rs`

- [x] 7. Node 테이블 생성 (x→y→z 정렬)

  **What to do**:
  - `node_table.py`에 Node 테이블 생성 함수 구현
  - 모든 node_i, node_j 좌표에서 중복 제거
  - x → y → z 우선순위로 정렬
  - ID 1부터 부여 (0 사용 안 함)
  - (node_id, x, y, z) 튜플 리스트 반환

  **Must NOT do**:
  - ID 0 사용
  - 정렬 순서 위반

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`python-code-style`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 8, 9)
  - **Blocks**: Task 10
  - **Blocked By**: Tasks 4-6

  **References**:
  - `devplandoc.md:37-39` - ID 정렬 규칙

  **Acceptance Criteria**:
  - [ ] 중복 노드 제거됨
  - [ ] ID 1부터 시작
  - [ ] x→y→z 정렬 순서 준수
  - [ ] pytest 테스트 통과

  **QA Scenarios**:
  ```
  Scenario: Node 테이블 생성
    Tool: Bash
    Steps:
      1. python -c "from node_table import create_node_table; nodes = create_node_table(df); print(nodes[0])"
    Expected Result: (1, 0, 0, 0) - 첫 번째 노드
    Evidence: .sisyphus/evidence/task-07-node-table.txt
  ```

  **Commit**: NO (Wave 3 완료 후 일괄)

- [x] 8. Element 테이블 생성 (Column/Girder 분류)

  **What to do**:
  - `element_table.py`에 Element 테이블 생성 함수 구현
  - 부재_ID → element_id 매핑
  - node_i, node_j ID 매핑 (node 테이블 참조)
  - member_type 분류: Column (수직) vs Girder (수평)
  - (element_id, node_i_id, node_j_id, member_type) 튜플 리스트 반환

  **Must NOT do**:
  - member_type 오타 (Column, Girder 정확히)
  - node_id 매핑 오류

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`python-code-style`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 7, 9)
  - **Blocks**: Task 10
  - **Blocked By**: Tasks 4-6, 7

  **References**:
  - `devplandoc.md:35` - element table 정의

  **Acceptance Criteria**:
  - [ ] 모든 부재 element_id 부여됨
  - [ ] Column/Girder 정확히 분류됨
  - [ ] node_id 매핑 정확
  - [ ] pytest 테스트 통과

  **QA Scenarios**:
  ```
  Scenario: Element 테이블 생성
    Tool: Bash
    Steps:
      1. python -c "from element_table import create_element_table; elems = create_element_table(df, nodes); print(elems[0])"
    Expected Result: (1, node_i_id, node_j_id, 'Column' 또는 'Girder')
    Evidence: .sisyphus/evidence/task-08-element-table.txt
  ```

  **Commit**: NO (Wave 3 완료 후 일괄)

- [x] 9. 기본 렌더링 구현 (grid, node, element)

  **What to do**:
  - `renderer.rs`에 렌더링 함수 구현
  - Grid lines: z=min(z) 레벨에 x, y 좌표 투영
  - Nodes: 작은 점으로 렌더링
  - Elements: 직선으로 렌더링
  - egui::Painter 사용

  **Must NOT do**:
  - 3D 렌더링 (2D 투영만 우선)
  - 복잡한 셰이더

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 7, 8)
  - **Blocks**: Task 12
  - **Blocked By**: Tasks 5

  **References**:
  - `devplandoc.md:78-80` - grid, node, element 렌더링

  **Acceptance Criteria**:
  - [ ] Grid lines 렌더링됨
  - [ ] Nodes 점으로 표시됨
  - [ ] Elements 직선으로 표시됨
  - [ ] cargo test 통과

  **QA Scenarios**:
  ```
  Scenario: 렌더링 확인
    Tool: Bash
    Steps:
      1. cd src/rust && cargo run --release
      2. 윈도우에 grid/node/element 표시 확인
    Expected Result: 렌더링됨 (exit code 0)
    Evidence: .sisyphus/evidence/task-09-render.txt
  ```

  **Commit**: YES (Wave 3 완료 후)
  - Message: `feat: add node/element table generation`
  - Files: `src/python/node_table.py`, `src/python/element_table.py`, `src/rust/src/graphics/renderer.rs`

- [ ] 10. 검증 로직 구현

  **What to do**:
  - `validators.py`에 검증 함수 구현
  - `validate_axis_parallel()`: 부재가 x/y/z 축과 평행한지
  - `validate_no_diagonal()`: 수평 부재가 대각선 아닌지
  - `validate_orphan_nodes()`: 연결되지 않은 노드 검출
  - `validate_duplicate_ids()`: 중복 ID 검출
  - `validate_zero_length()`: 길이 0 부재 검출
  - `validate_overlapping()`: 중복 부재 검출
  - `validate_floor_level()`: 바닥층 레벨 일관성

  **Must NOT do**:
  - 검증 없이 렌더링 허용
  - 모호한 에러 메시지

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`python-code-style`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 4 (with Tasks 11, 12)
  - **Blocks**: Task 13
  - **Blocked By**: Tasks 7-8

  **References**:
  - `devplandoc.md:44-61` - 검증 규칙

  **Acceptance Criteria**:
  - [ ] 모든 검증 함수 구현됨
  - [ ] 검증 실패 시 명확한 에러 메시지
  - [ ] pytest 테스트 통과

  **QA Scenarios**:
  ```
  Scenario: 검증 성공
    Tool: Bash
    Steps:
      1. python -c "from validators import validate_all; validate_all(nodes, elements)"
    Expected Result: 예외 없음
    Evidence: .sisyphus/evidence/task-10-validate-pass.txt

  Scenario: 검증 실패 (orphan node)
    Tool: Bash
    Steps:
      1. python -c "from validators import validate_orphan_nodes; validate_orphan_nodes(bad_nodes, elements)"
    Expected Result: ValueError: "Orphan nodes detected: [...]"
    Evidence: .sisyphus/evidence/task-10-validate-fail.txt
  ```

  **Commit**: NO (Wave 4 완료 후 일괄)

- [ ] 11. PyO3 바인딩 구현

  **What to do**:
  - `lib.rs`에 PyO3 함수 노출
  - `load_and_validate(path: &str) -> PyResult<Data>`
  - `render_data(data: &Data)`
  - Python에서 호출 가능하도록 설정

  **Must NOT do**:
  - 복잡한 양방향 동기화
  - GIL 블로킹

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 4 (with Tasks 10, 12)
  - **Blocks**: Task 13
  - **Blocked By**: Tasks 2, 9

  **References**:
  - PyO3 docs: https://pyo3.rs/

  **Acceptance Criteria**:
  - [ ] `maturin develop` 성공
  - [ ] Python에서 Rust 함수 호출 가능
  - [ ] cargo test 통과

  **QA Scenarios**:
  ```
  Scenario: PyO3 호출
    Tool: Bash
    Steps:
      1. maturin develop --release
      2. python -c "import assyplan; data = assyplan.load_and_validate('data.txt'); print(len(data.nodes))"
    Expected Result: 노드 수 출력
    Evidence: .sisyphus/evidence/task-11-pyo3.txt
  ```

  **Commit**: NO (Wave 4 완료 후 일괄)

- [ ] 12. UI 레이아웃 + 버튼 구현

  **What to do**:
  - `ui.rs`에 UI 레이아웃 구현
  - 상단: 헤더 영역 (버튼 배치)
  - 좌측: 상태 표시창
  - 우측: 뷰 윈도우 (탭: 환경설정/View/Result)
  - 버튼: recalc, reset, mode toggle
  - ID 라벨 on/off 토글

  **Must NOT do**:
  - Simulation Mode 활성화 (버튼만 표시)
  - 복잡한 애니메이션

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 4 (with Tasks 10, 11)
  - **Blocks**: Task 13
  - **Blocked By**: Tasks 9

  **References**:
  - `devplandoc.md:70-87` - UI 레이아웃

  **Acceptance Criteria**:
  - [ ] 레이아웃 구조 구현됨
  - [ ] recalc 버튼: 검증 → 렌더링
  - [ ] reset 버튼: 초기화
  - [ ] mode toggle: Simulation 비활성
  - [ ] ID 라벨 on/off 동작
  - [ ] cargo test 통과

  **QA Scenarios**:
  ```
  Scenario: 버튼 동작
    Tool: Bash
    Steps:
      1. cd src/rust && cargo run --release
      2. recalc 버튼 클릭
      3. reset 버튼 클릭
    Expected Result: 버튼 동작함 (exit code 0)
    Evidence: .sisyphus/evidence/task-12-buttons.txt
  ```

  **Commit**: YES (Wave 4 완료 후)
  - Message: `feat: add validation and PyO3 integration`
  - Files: `src/python/validators.py`, `src/rust/src/lib.rs`, `src/rust/src/graphics/ui.rs`

- [ ] 13. 통합 테스트 구현

  **What to do**:
  - `tests/test_integration.py` 작성
  - E2E 테스트: CSV 로드 → 검증 → 렌더링
  - 에러 케이스 테스트: 잘못된 CSV, 검증 실패

  **Must NOT do**:
  - 수동 테스트만 의존

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`python-code-style`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 5 (with Tasks 14, 15)
  - **Blocks**: FINAL
  - **Blocked By**: Tasks 10-12

  **References**:
  - `data.txt` - 샘플 데이터

  **Acceptance Criteria**:
  - [ ] E2E 테스트 통과
  - [ ] 에러 케이스 테스트 통과
  - [ ] pytest 통과

  **QA Scenarios**:
  ```
  Scenario: E2E 테스트
    Tool: Bash
    Steps:
      1. pytest tests/test_integration.py -v
    Expected Result: 모든 테스트 통과
    Evidence: .sisyphus/evidence/task-13-e2e.txt
  ```

  **Commit**: NO (Wave 5 완료 후 일괄)

- [ ] 14. SKILLS.md 작성

  **What to do**:
  - 프로젝트 루트에 `SKILLS.md` 작성
  - 개발 1단계 구현 내용 정리
  - 사용된 기술 스택, 패턴, 주의사항 문서화

  **Must NOT do**:
  - 과도한 문서화 (핵심만)

  **Recommended Agent Profile**:
  - **Category**: `writing`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 5 (with Tasks 13, 15)
  - **Blocks**: FINAL
  - **Blocked By**: Tasks 10-12

  **References**:
  - `devplandoc.md:95-97` - SKILLS.md 요구사항

  **Acceptance Criteria**:
  - [ ] `SKILLS.md` 존재
  - [ ] 기술 스택 설명 포함
  - [ ] 구현 패턴 설명 포함

  **QA Scenarios**:
  ```
  Scenario: SKILLS.md 확인
    Tool: Bash
    Steps:
      1. cat SKILLS.md | grep "eframe"
    Expected Result: eframe 설명 존재
    Evidence: .sisyphus/evidence/task-14-skills.txt
  ```

  **Commit**: NO (Wave 5 완료 후 일괄)

- [ ] 15. 최종 검증

  **What to do**:
  - 모든 테스트 실행: pytest + cargo test
  - 샘플 데이터로 전체 파이프라인 실행
  - 코드 품질 검사: 린터, 포맷터

  **Must NOT do**:
  - 테스트 실패 무시

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 5 (with Tasks 13, 14)
  - **Blocks**: FINAL
  - **Blocked By**: Tasks 10-12

  **References**:
  - None

  **Acceptance Criteria**:
  - [ ] pytest 통과
  - [ ] cargo test 통과
  - [ ] 샘플 데이터 렌더링 성공

  **QA Scenarios**:
  ```
  Scenario: 최종 검증
    Tool: Bash
    Steps:
      1. pytest tests/ -v
      2. cd src/rust && cargo test
      3. maturin develop --release
      4. python -c "import assyplan; assyplan.run('data.txt')"
    Expected Result: 모든 단계 성공
    Evidence: .sisyphus/evidence/task-15-final.txt
  ```

  **Commit**: YES (Wave 5 완료 후)
  - Message: `feat: complete development mode implementation`
  - Files: `tests/`, `SKILLS.md`

---

## Final Verification Wave

- [ ] F1. **Plan Compliance Audit** — `oracle`
  모든 "Must Have" 구현 확인, "Must NOT Have" 위반 없음 확인

- [ ] F2. **Code Quality Review** — `unspecified-high`
  `pytest` + `cargo test` 통과, 린터 경고 없음

- [ ] F3. **Real Manual QA** — `unspecified-high` (+ `playwright` skill)
  샘플 CSV 로드 → 검증 → 3D 렌더링 → recalc/reset 동작 확인

- [ ] F4. **Scope Fidelity Check** — `deep`
  계획 범위 준수, 과도한 기능 추가 없음 확인

---

## Commit Strategy

- **Wave 1 완료**: `chore: initial project scaffold`
- **Wave 2 완료**: `feat: add data parser and graphics core`
- **Wave 3 완료**: `feat: add node/element table generation`
- **Wave 4 완료**: `feat: add validation and PyO3 integration`
- **Wave 5 완료**: `feat: complete development mode implementation`

---

## Success Criteria

### Verification Commands

```bash
# Python 테스트
pytest tests/ -v

# Rust 테스트
cargo test

# 통합 실행
maturin develop --release
python -c "import assyplan; print(assyplan.load_csv('data.txt'))"
```

### Final Checklist

- [ ] CSV 로드 성공 (UTF-8 + EUC-KR)
- [ ] Node 테이블: ID 1부터, x→y→z 정렬
- [ ] Element 테이블: Column/Girder 분류
- [ ] 검증: orphan, 중복, zero-length, 축 평행
- [ ] 3D 뷰: grid, node, element 렌더링
- [ ] 뷰 컨트롤: X-Y/Y-Z/Z-X, Orbit, Zoom, Pan
- [ ] ID 라벨 on/off
- [ ] recalc 버튼: 검증 → 렌더링
- [ ] reset 버튼: 초기화
- [ ] mode toggle: Simulation 비활성
- [ ] pytest 통과
- [ ] cargo test 통과
