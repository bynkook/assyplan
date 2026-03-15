# 개발 2단계 최종 계획서

## TL;DR

> **목표**: 개발 모드(Development Mode)에 시공단계(Step) 시각화 기능 추가
> 
> **산출물**:
> - Python: 적합/안정 조건 검증, Construction Sequence Table, Workfront Step Table
> - Rust: Step 탐색 UI (이전/다음/슬라이더/직접입력), 누적 렌더링 + 캐시
> - 테스트: pytest + cargo test 확장
> 
> **예상 기간**: 2-3일
> **병렬 실행**: YES - 5 Waves
> **Critical Path**: 선행부재 파싱 → 안정조건 검증 → Sequence Table → Step Table → UI/렌더링

---

## Context

### 원본 요청
`devplandoc.md:101-143`의 개발 2단계 요구사항

### 현재 상태 (Phase 1 완료)

| 구현됨 | 파일 |
|--------|------|
| CSV 로드 + `선행부재ID` 컬럼 파싱 | `data_loader.py` |
| Node/Element 테이블 생성 | `node_table.py`, `element_table.py` |
| 기본 검증 (orphan, 중복, 축평행 등) | `validators.py` |
| 2D 렌더링 (grid, node, element) | `renderer.rs` |
| UI 레이아웃 (header, tabs, status) | `ui.rs` |
| PyO3 바인딩 | `lib.rs` |

### 핵심 설계 결정

1. **선행부재 그래프**: `선행부재ID`로 DAG(Directed Acyclic Graph) 구성
2. **Workfront 식별**: `선행부재ID`가 없는 부재 = 새로운 workfront 시작점
3. **Step 할당**: 위상정렬(topological sort) + 안정 조건 검사
4. **누적 렌더링**: Step N 렌더링 시 Step 1~N-1 부재 포함

### 샘플 데이터 분석 (`data.txt`)

- **259개 부재**: 105 Columns + 154 Girders
- **선행부재ID 패턴**: 
  - `1CF001`: 선행부재 없음 → Workfront 시작점
  - `2GF001`: `1CF001` 다음 → 동일 workfront
- **층 구조**: 7개 층 (z=0, 10000, 20000, ..., 70000)

---

## Work Objectives

### Core Objective
사용자 입력 데이터의 `선행부재ID`를 기반으로 Construction Sequence Table과 Workfront Step Table을 생성하고, Step별 누적 렌더링 UI 제공

### Concrete Deliverables

1. `src/python/sequence_table.py` - Construction Sequence Table 생성
2. `src/python/step_table.py` - Workfront Step Table 생성
3. `src/python/stability_validators.py` - 적합/안정 조건 검증
4. `src/rust/src/graphics/step_renderer.rs` - Step별 누적 렌더링 + 캐시
5. UI 확장: Step 탐색 컨트롤 (이전/다음/슬라이더/직접입력)
6. `tests/python/test_phase2.py` - Phase 2 테스트

### Definition of Done

- [ ] 선행부재 DAG 파싱 및 순환 참조 검출
- [ ] Workfront 자동 식별 (선행부재 없는 부재 위치)
- [ ] Construction Sequence Table 생성 (workfront_id, member_id)
- [ ] Workfront Step Table 생성 (workfront_id, step, member_id)
- [ ] 적합/안정 조건 검증 (3기둥+2거더 규칙)
- [ ] Step 탐색 UI (이전/다음 버튼, 슬라이더, 직접입력)
- [ ] 누적 렌더링 (Step N = Step 1~N 부재 표시)
- [ ] 캐시 전략 구현 (1만개 부재 60fps)
- [ ] pytest + cargo test 통과

### Must Have

- Construction Sequence Table 생성
- Workfront Step Table 생성
- 적합/안정 조건 검증
- Step 탐색 UI (이전/다음/슬라이더/직접입력)
- 누적 렌더링
- recalc/reset 버튼 동작 (Step 초기화 포함)

### Must NOT Have (Guardrails)

- Simulation Mode 활성화 (버튼만 표시, 비활성 유지)
- 자동 Step 생성 알고리즘 (Phase 3)
- Workfront 확장 방향 선택 (Phase 3)
- ID 0 사용

---

## Verification Strategy

### Test Decision

- **Infrastructure exists**: YES (Phase 1에서 pytest + cargo test 구축)
- **Automated tests**: YES (TDD 지속)
- **Framework**: pytest (Python) + cargo test (Rust)

### QA Policy

모든 태스크는 Agent-Executed QA Scenarios 포함:
- **Python Module**: pytest 테스트
- **Rust Module**: cargo test
- **Integration**: E2E 파이프라인 테스트

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately - 데이터 구조):
├── Task 1: 선행부재 관계 파싱 + DAG 구성 [quick]
├── Task 2: Workfront 식별 로직 [quick]
└── Task 3: 적합/안정 조건 검증 함수 [quick]

Wave 2 (After Wave 1 - 테이블 생성):
├── Task 4: Construction Sequence Table 생성 [quick]
├── Task 5: Workfront Step Table 생성 [quick]
└── Task 6: Step 할당 알고리즘 (위상정렬 기반) [quick]

Wave 3 (After Wave 2 - Rust 확장):
├── Task 7: StepRenderData 구조체 정의 [quick]
├── Task 8: 누적 렌더링 로직 구현 [quick]
└── Task 9: 렌더링 캐시 전략 구현 [quick]

Wave 4 (After Wave 3 - UI):
├── Task 10: Step 탐색 UI (이전/다음 버튼) [quick]
├── Task 11: Step 슬라이더 + 직접입력 [quick]
└── Task 12: PyO3 바인딩 확장 [quick]

Wave 5 (After Wave 4 - 통합):
├── Task 13: Python 통합 테스트 [quick]
├── Task 14: Rust 통합 테스트 [quick]
└── Task 15: SKILLS.md 업데이트 [writing]

Critical Path: 1 → 4 → 6 → 7 → 8 → 12 → 13
Parallel Speedup: ~50% faster than sequential
Max Concurrent: 3
```

### Dependency Matrix

| Wave | Depends On | Blocks |
|------|------------|--------|
| 1-3 | Phase 1 | 4-6 |
| 4-6 | 1-3 | 7-9 |
| 7-9 | 4-6 | 10-12 |
| 10-12 | 7-9 | 13-15 |
| 13-15 | 10-12 | FINAL |

---

## TODOs

### Wave 1: 데이터 구조

- [ ] 1. 선행부재 관계 파싱 + DAG 구성

  **What to do**:
  - `src/python/precedent_graph.py` 생성
  - `선행부재ID` 컬럼에서 부재 간 선행관계 추출
  - DAG(Directed Acyclic Graph) 데이터 구조 구성
  - 순환 참조(cycle) 검출 및 예외 발생

  **Must NOT do**:
  - 자동 Step 생성 (Phase 3)
  - Workfront 식별 (Task 2)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`python-code-style`, `python-design-patterns`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2, 3)
  - **Blocks**: Tasks 4-6
  - **Blocked By**: None

  **References**:
  - `devplandoc.md:216` - 선행부재 정의
  - `data_loader.py:15` - `선행부재ID` 컬럼 존재 확인

  **Acceptance Criteria**:
  - [ ] DAG 구성 함수 구현
  - [ ] 순환 참조 검출 시 ValueError 발생
  - [ ] pytest 테스트 통과

  **QA Scenarios**:
  ```
  Scenario: DAG 구성
    Tool: Bash
    Steps:
      1. python -c "from precedent_graph import build_dag; dag = build_dag(df); print(len(dag.nodes))"
    Expected Result: 259 (부재 수)

  Scenario: 순환 참조 검출
    Tool: Bash
    Steps:
      1. python -c "from precedent_graph import build_dag; build_dag(cyclic_df)"
    Expected Result: ValueError: "Cycle detected"
  ```

  **Commit**: NO (Wave 1 완료 후 일괄)

- [ ] 2. Workfront 식별 로직

  **What to do**:
  - `src/python/workfront.py` 생성
  - `선행부재ID`가 비어있는 부재 = 새로운 workfront 시작점
  - Workfront ID 자동 할당 (1부터 시작)
  - 각 workfront에 속한 부재 목록 반환

  **Must NOT do**:
  - Workfront 확장 방향 선택 (Phase 3)
  - 여러 workfront 간 우선순위 설정

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`python-code-style`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3)
  - **Blocks**: Tasks 4-6
  - **Blocked By**: None

  **References**:
  - `devplandoc.md:192-207` - workfront 정의
  - `devplandoc.md:198` - 개발모드 workfront 식별 규칙

  **Acceptance Criteria**:
  - [ ] Workfront 시작점 식별 함수 구현
  - [ ] Workfront ID 1부터 할당
  - [ ] pytest 테스트 통과

  **QA Scenarios**:
  ```
  Scenario: Workfront 식별
    Tool: Bash
    Steps:
      1. python -c "from workfront import identify_workfronts; wfs = identify_workfronts(df); print(len(wfs))"
    Expected Result: Workfront 수 출력 (샘플 데이터 기준 1개 예상)
  ```

  **Commit**: NO (Wave 1 완료 후 일괄)

- [ ] 3. 적합/안정 조건 검증 함수

  **What to do**:
  - `src/python/stability_validators.py` 생성
  - `validate_minimum_assembly()`: 최소 조립 단위 (기둥 3개 + 거더 2개, 90도 배치) 검증
  - `validate_column_support()`: 기둥 i노드가 바닥층 또는 하부 기둥 j노드와 연결 확인
  - `validate_girder_support()`: 거더 양쪽 노드가 안정 판정 받은 기둥/거더와 연결 확인
  - `validate_no_ground_girder()`: z=0 레벨에 거더 없음 확인

  **Must NOT do**:
  - Step별 검증 (Task 6에서 처리)
  - 검증 실패 시 자동 수정

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`python-code-style`, `python-design-patterns`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2)
  - **Blocks**: Tasks 4-6
  - **Blocked By**: None

  **References**:
  - `devplandoc.md:220-244` - 적합 및 안정 조건
  - `devplandoc.md:228-236` - 부재 생성 조건

  **Acceptance Criteria**:
  - [ ] 4개 검증 함수 구현
  - [ ] 각 검증 실패 시 명확한 에러 메시지
  - [ ] pytest 테스트 통과

  **QA Scenarios**:
  ```
  Scenario: 최소 조립 단위 검증
    Tool: Bash
    Steps:
      1. python -c "from stability_validators import validate_minimum_assembly; validate_minimum_assembly(nodes, elements)"
    Expected Result: True 또는 ValueError

  Scenario: 바닥층 거더 검출
    Tool: Bash
    Steps:
      1. python -c "from stability_validators import validate_no_ground_girder; validate_no_ground_girder(elements_with_z0_girder)"
    Expected Result: ValueError: "Ground level girder detected"
  ```

  **Commit**: YES (Wave 1 완료 후)
  - Message: `feat: add precedent graph and stability validators`
  - Files: `src/python/precedent_graph.py`, `src/python/workfront.py`, `src/python/stability_validators.py`

### Wave 2: 테이블 생성

- [ ] 4. Construction Sequence Table 생성

  **What to do**:
  - `src/python/sequence_table.py` 생성
  - DAG 위상정렬로 부재 생성 순서 결정
  - `(workfront_id, member_id)` 튜플 리스트 생성
  - CSV/JSON 저장 함수

  **Must NOT do**:
  - Step 할당 (Task 6)
  - 적합 조건 검증 (Task 3에서 처리)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`python-code-style`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 5, 6)
  - **Blocks**: Tasks 7-9
  - **Blocked By**: Tasks 1-3

  **References**:
  - `devplandoc.md:111` - construction sequence table 정의
  - `devplandoc.md:125` - 컬럼 정의

  **Acceptance Criteria**:
  - [ ] 위상정렬 기반 순서 결정
  - [ ] workfront_id, member_id 컬럼 포함
  - [ ] CSV/JSON 저장 가능
  - [ ] pytest 테스트 통과

  **QA Scenarios**:
  ```
  Scenario: Sequence Table 생성
    Tool: Bash
    Steps:
      1. python -c "from sequence_table import create_sequence_table; seq = create_sequence_table(dag, workfronts); print(seq[:5])"
    Expected Result: [(1, member_id), ...] 형태 출력
  ```

  **Commit**: NO (Wave 2 완료 후 일괄)

- [ ] 5. Workfront Step Table 생성

  **What to do**:
  - `src/python/step_table.py` 생성
  - Construction Sequence를 Step 단위로 그룹화
  - `(workfront_id, step, member_id)` 튜플 리스트 생성
  - 동일 step에 여러 member 허용
  - CSV/JSON 저장 함수

  **Must NOT do**:
  - 적합 조건 검증 없이 Step 할당
  - Step 0 사용 (1부터 시작)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`python-code-style`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 4, 6)
  - **Blocks**: Tasks 7-9
  - **Blocked By**: Tasks 1-3

  **References**:
  - `devplandoc.md:115-116` - workfront step table 정의
  - `devplandoc.md:127-131` - 컬럼 정의, integer 규칙

  **Acceptance Criteria**:
  - [ ] Step 1부터 시작
  - [ ] 동일 step에 복수 member 허용
  - [ ] workfront_id, step, member_id 컬럼 포함
  - [ ] pytest 테스트 통과

  **QA Scenarios**:
  ```
  Scenario: Step Table 생성
    Tool: Bash
    Steps:
      1. python -c "from step_table import create_step_table; steps = create_step_table(sequence, validators); print(steps[:10])"
    Expected Result: [(1, 1, member_id), (1, 1, member_id2), ...] 형태 출력
  ```

  **Commit**: NO (Wave 2 완료 후 일괄)

- [ ] 6. Step 할당 알고리즘 (위상정렬 + 안정조건)

  **What to do**:
  - Construction Sequence의 각 부재에 Step 번호 할당
  - 안정 조건 통과 시점에서 Step 증가
  - 이미 안정 판정 받은 부재는 재검증 스킵 (성능 최적화)
  - Double-check: 각 Step 완료 후 전체 모델 안정 조건 검증

  **Must NOT do**:
  - Workfront 간 Step 동기화 (각 workfront 독립)
  - Step 0 사용

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`python-code-style`, `python-design-patterns`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 4, 5)
  - **Blocks**: Tasks 7-9
  - **Blocked By**: Tasks 1-3

  **References**:
  - `devplandoc.md:117-119` - double check, 반복 제외 로직
  - `devplandoc.md:186-188` - step 정의

  **Acceptance Criteria**:
  - [ ] 위상정렬 순서 유지
  - [ ] 안정 조건 기반 Step 경계 결정
  - [ ] 이미 안정 판정 받은 부재 스킵
  - [ ] pytest 테스트 통과

  **QA Scenarios**:
  ```
  Scenario: Step 할당
    Tool: Bash
    Steps:
      1. python -c "from step_table import assign_steps; result = assign_steps(sequence, nodes, elements); print(max(s[1] for s in result))"
    Expected Result: 최대 step 번호 출력
  ```

  **Commit**: YES (Wave 2 완료 후)
  - Message: `feat: add construction sequence and step tables`
  - Files: `src/python/sequence_table.py`, `src/python/step_table.py`

### Wave 3: Rust 확장

- [ ] 7. StepRenderData 구조체 정의

  **What to do**:
  - `src/rust/src/graphics/step_renderer.rs` 생성
  - `StepRenderData`: 전체 데이터 + Step별 부재 인덱스
  - `current_step`, `max_step` 필드
  - Step별 부재 필터링 메서드

  **Must NOT do**:
  - 렌더링 로직 구현 (Task 8)
  - UI 구현 (Task 10-11)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`rust-desktop-applications`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 8, 9)
  - **Blocks**: Tasks 10-12
  - **Blocked By**: Tasks 4-6

  **References**:
  - `renderer.rs` - 기존 RenderData 구조
  - `devplandoc.md:133-135` - Step별 렌더링 요구사항

  **Acceptance Criteria**:
  - [ ] StepRenderData 구조체 정의
  - [ ] Step별 부재 필터링 메서드
  - [ ] cargo test 통과

  **QA Scenarios**:
  ```
  Scenario: StepRenderData 테스트
    Tool: Bash
    Steps:
      1. cd src/rust && cargo test step_renderer
    Expected Result: 모든 테스트 통과
  ```

  **Commit**: NO (Wave 3 완료 후 일괄)

- [ ] 8. 누적 렌더링 로직 구현

  **What to do**:
  - `render_step(step: usize)`: Step N까지의 모든 부재 렌더링
  - 이전 Step 부재는 회색/투명도 조절로 구분
  - 현재 Step 부재는 원래 색상 (Column: 빨강, Girder: 초록)
  - Grid, Node는 항상 전체 표시

  **Must NOT do**:
  - 캐시 구현 (Task 9)
  - UI 구현 (Task 10-11)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`rust-desktop-applications`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 7, 9)
  - **Blocks**: Tasks 10-12
  - **Blocked By**: Tasks 4-6

  **References**:
  - `renderer.rs:184-207` - 기존 render_elements 로직
  - `devplandoc.md:133` - 누적 렌더링 요구사항

  **Acceptance Criteria**:
  - [ ] Step N까지 누적 렌더링
  - [ ] 이전/현재 Step 시각적 구분
  - [ ] cargo test 통과

  **QA Scenarios**:
  ```
  Scenario: 누적 렌더링 테스트
    Tool: Bash
    Steps:
      1. cd src/rust && cargo test cumulative_render
    Expected Result: 모든 테스트 통과
  ```

  **Commit**: NO (Wave 3 완료 후 일괄)

- [ ] 9. 렌더링 캐시 전략 구현

  **What to do**:
  - Step별 렌더링 결과 캐시 (egui::TextureHandle 또는 메시 캐시)
  - Step 변경 시 증분 렌더링 (변경된 부분만 업데이트)
  - 1만개 부재 60fps 목표
  - 메모리 사용량 모니터링

  **Must NOT do**:
  - 무한 캐시 (메모리 제한 고려)
  - GPU 직접 접근 (eframe/wgpu 추상화 사용)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`rust-desktop-applications`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 7, 8)
  - **Blocks**: Tasks 10-12
  - **Blocked By**: Tasks 4-6

  **References**:
  - `devplandoc.md:135` - 60fps 요구사항
  - eframe 캐시 패턴

  **Acceptance Criteria**:
  - [ ] Step 변경 시 증분 렌더링
  - [ ] 1만개 부재 60fps 달성
  - [ ] cargo test 통과

  **QA Scenarios**:
  ```
  Scenario: 캐시 성능 테스트
    Tool: Bash
    Steps:
      1. cd src/rust && cargo test --release cache_performance
    Expected Result: 60fps 이상
  ```

  **Commit**: YES (Wave 3 완료 후)
  - Message: `feat: add step-based cumulative rendering with cache`
  - Files: `src/rust/src/graphics/step_renderer.rs`, `src/rust/src/graphics/mod.rs`

### Wave 4: UI

- [ ] 10. Step 탐색 UI (이전/다음 버튼)

  **What to do**:
  - `ui.rs`의 `UiState`에 `current_step`, `max_step` 필드 추가
  - 헤더 또는 View 탭에 이전(◀)/다음(▶) 버튼 추가
  - 버튼 클릭 시 `current_step` 변경
  - Step 1 미만, max_step 초과 방지

  **Must NOT do**:
  - 슬라이더 구현 (Task 11)
  - 직접입력 구현 (Task 11)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`rust-desktop-applications`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 4 (with Tasks 11, 12)
  - **Blocks**: Tasks 13-15
  - **Blocked By**: Tasks 7-9

  **References**:
  - `ui.rs:56-108` - 기존 render_header 구조
  - `devplandoc.md:133` - 화살표 클릭 요구사항

  **Acceptance Criteria**:
  - [ ] 이전/다음 버튼 표시
  - [ ] 버튼 클릭 시 Step 변경
  - [ ] 경계값 검증 (1 ≤ step ≤ max)
  - [ ] cargo test 통과

  **QA Scenarios**:
  ```
  Scenario: Step 버튼 테스트
    Tool: Bash
    Steps:
      1. cd src/rust && cargo test step_navigation
    Expected Result: 모든 테스트 통과
  ```

  **Commit**: NO (Wave 4 완료 후 일괄)

- [ ] 11. Step 슬라이더 + 직접입력

  **What to do**:
  - egui::Slider로 Step 범위 (1 ~ max_step) 선택
  - egui::DragValue 또는 TextEdit로 Step 직접입력
  - 입력값 검증 (정수, 범위 확인)
  - 현재 Step / 최대 Step 표시 라벨

  **Must NOT do**:
  - Step 0 허용
  - 비정수 입력 허용

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`rust-desktop-applications`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 4 (with Tasks 10, 12)
  - **Blocks**: Tasks 13-15
  - **Blocked By**: Tasks 7-9

  **References**:
  - `devplandoc.md:133` - 직접입력 이동 요구사항
  - egui Slider/DragValue 문서

  **Acceptance Criteria**:
  - [ ] 슬라이더 1 ~ max_step 범위
  - [ ] 직접입력 가능
  - [ ] 입력값 검증
  - [ ] cargo test 통과

  **QA Scenarios**:
  ```
  Scenario: Step 슬라이더 테스트
    Tool: Bash
    Steps:
      1. cd src/rust && cargo test step_slider
    Expected Result: 모든 테스트 통과
  ```

  **Commit**: NO (Wave 4 완료 후 일괄)

- [ ] 12. PyO3 바인딩 확장

  **What to do**:
  - `lib.rs`에 Step Table 데이터 전달 함수 추가
  - `PyStepTable`: Step Table Python 래퍼
  - `load_step_data(step_table) -> PyResult<()>`
  - `get_current_step() -> usize`, `set_current_step(step: usize)`

  **Must NOT do**:
  - 양방향 동기화 (Python → Rust 단방향)
  - GIL 블로킹

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`rust-desktop-applications`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 4 (with Tasks 10, 11)
  - **Blocks**: Tasks 13-15
  - **Blocked By**: Tasks 7-9

  **References**:
  - `lib.rs` - 기존 PyO3 바인딩 패턴
  - PyO3 문서

  **Acceptance Criteria**:
  - [ ] Step Table Python 전달 가능
  - [ ] Step 조회/설정 함수
  - [ ] maturin develop 성공
  - [ ] cargo test 통과

  **QA Scenarios**:
  ```
  Scenario: PyO3 Step 바인딩
    Tool: Bash
    Steps:
      1. maturin develop --release
      2. python -c "import assyplan; assyplan.load_step_data(step_table)"
    Expected Result: 성공
  ```

  **Commit**: YES (Wave 4 완료 후)
  - Message: `feat: add step navigation UI and PyO3 bindings`
  - Files: `src/rust/src/graphics/ui.rs`, `src/rust/src/lib.rs`

### Wave 5: 통합

- [ ] 13. Python 통합 테스트

  **What to do**:
  - `tests/python/test_phase2.py` 생성
  - E2E: CSV 로드 → DAG → Workfront → Sequence → Step Table
  - 에러 케이스: 순환 참조, 안정 조건 실패
  - 샘플 데이터(`data.txt`)로 전체 파이프라인 테스트

  **Must NOT do**:
  - Rust 테스트 (Task 14)
  - 성능 테스트 (별도)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`python-code-style`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 5 (with Tasks 14, 15)
  - **Blocks**: FINAL
  - **Blocked By**: Tasks 10-12

  **References**:
  - `tests/python/test_integration.py` - Phase 1 테스트 패턴

  **Acceptance Criteria**:
  - [ ] E2E 테스트 통과
  - [ ] 에러 케이스 테스트 통과
  - [ ] pytest 통과

  **QA Scenarios**:
  ```
  Scenario: Phase 2 E2E
    Tool: Bash
    Steps:
      1. pytest tests/python/test_phase2.py -v
    Expected Result: 모든 테스트 통과
  ```

  **Commit**: NO (Wave 5 완료 후 일괄)

- [ ] 14. Rust 통합 테스트

  **What to do**:
  - Step 렌더링 통합 테스트
  - UI State + StepRenderData 연동 테스트
  - 캐시 무효화 테스트

  **Must NOT do**:
  - Python 테스트 (Task 13)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: [`rust-desktop-applications`]

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 5 (with Tasks 13, 15)
  - **Blocks**: FINAL
  - **Blocked By**: Tasks 10-12

  **References**:
  - `renderer.rs:216-276` - 기존 Rust 테스트 패턴

  **Acceptance Criteria**:
  - [ ] Step 렌더링 테스트 통과
  - [ ] UI 연동 테스트 통과
  - [ ] cargo test 통과

  **QA Scenarios**:
  ```
  Scenario: Rust 통합 테스트
    Tool: Bash
    Steps:
      1. cd src/rust && cargo test
    Expected Result: 모든 테스트 통과
  ```

  **Commit**: NO (Wave 5 완료 후 일괄)

- [ ] 15. SKILLS.md 업데이트 (Phase 2)

  **What to do**:
  - `.opencode/skills/dev-phase2/SKILLS.md` 생성
  - Phase 2 구현 내용 정리
  - 선행부재 DAG, Step 할당 알고리즘 문서화
  - 캐시 전략 설명

  **Must NOT do**:
  - Phase 1 내용 중복
  - 과도한 문서화

  **Recommended Agent Profile**:
  - **Category**: `writing`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 5 (with Tasks 13, 14)
  - **Blocks**: FINAL
  - **Blocked By**: Tasks 10-12

  **References**:
  - `.opencode/skills/dev-phase1/SKILLS.md` - Phase 1 SKILLS 패턴
  - `AGENTS.md` - SKILLS 경로 규칙

  **Acceptance Criteria**:
  - [ ] `.opencode/skills/dev-phase2/SKILLS.md` 존재
  - [ ] Phase 2 기술 스택 설명 포함
  - [ ] 알고리즘 설명 포함

  **QA Scenarios**:
  ```
  Scenario: SKILLS.md 확인
    Tool: Bash
    Steps:
      1. test -f .opencode/skills/dev-phase2/SKILLS.md && echo "exists"
    Expected Result: exists
  ```

  **Commit**: YES (Wave 5 완료 후)
  - Message: `feat: complete phase 2 step visualization`
  - Files: `tests/`, `.opencode/skills/dev-phase2/SKILLS.md`

---

## Final Verification Wave

- [ ] F1. **Plan Compliance Audit**
  모든 "Must Have" 구현 확인, "Must NOT Have" 위반 없음 확인

- [ ] F2. **Code Quality Review**
  `pytest` + `cargo test` 통과, 린터 경고 없음

- [ ] F3. **Performance Verification**
  1만개 부재 렌더링 시 60fps 달성 확인

- [ ] F4. **Scope Fidelity Check**
  계획 범위 준수, Simulation Mode 비활성 유지 확인

---

## Commit Strategy

- **Wave 1 완료**: `feat: add precedent graph and stability validators`
- **Wave 2 완료**: `feat: add construction sequence and step tables`
- **Wave 3 완료**: `feat: add step-based cumulative rendering with cache`
- **Wave 4 완료**: `feat: add step navigation UI and PyO3 bindings`
- **Wave 5 완료**: `feat: complete phase 2 step visualization`

---

## Success Criteria

### Verification Commands

```bash
# Python 테스트
pytest tests/ -v

# Rust 테스트
cd src/rust && cargo test

# 통합 실행
maturin develop --release
python -c "
from data_loader import load_csv
from precedent_graph import build_dag
from workfront import identify_workfronts
from sequence_table import create_sequence_table
from step_table import create_step_table
df = load_csv('data.txt')
dag = build_dag(df)
wfs = identify_workfronts(df)
seq = create_sequence_table(dag, wfs)
steps = create_step_table(seq)
print(f'Total steps: {max(s[1] for s in steps)}')
"
```

### Final Checklist

- [ ] 선행부재 DAG 파싱 (순환 참조 검출)
- [ ] Workfront 자동 식별
- [ ] Construction Sequence Table 생성
- [ ] Workfront Step Table 생성
- [ ] 적합/안정 조건 검증 (3기둥+2거더)
- [ ] Step 탐색 UI (이전/다음/슬라이더/직접입력)
- [ ] 누적 렌더링 (이전 Step 회색 표시)
- [ ] 캐시 전략 (60fps)
- [ ] recalc/reset 버튼 동작
- [ ] Simulation Mode 비활성 유지
- [ ] pytest 통과
- [ ] cargo test 통과
