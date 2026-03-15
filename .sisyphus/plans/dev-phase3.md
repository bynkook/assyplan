# 개발 3단계 계획서 (개요)

## TL;DR

> **목표**: 시뮬레이션 모드 구현 - 자동 Step 생성 및 다중 시나리오 실행
> 
> **산출물**:
> - Simulation Mode 활성화
> - Grid 기반 자동 Node/Element 생성
> - Multiple Workfront 시뮬레이션
> - 시나리오 생성 알고리즘
> - Metrics & Plots
> 
> **예상 기간**: 3-4일
> **선행 조건**: 개발 1, 2단계 완료

---

## Context

### 원본 요청
`devplandoc.md:143-178`의 개발 3단계 요구사항

### 핵심 기능

1. **Simulation Mode 활성화**
   - Mode toggle 버튼 활성화
   - Development ↔ Simulation 전환

2. **Grid 기반 자동 생성**
   - x, y 방향 Grid Line 갯수 설정
   - z 레벨 간격 설정
   - Node/Element Table 자동 생성

3. **Multiple Workfront 설정**
   - Grid 교차점 선택으로 workfront 설정
   - 1개 이상 workfront 지원

4. **시나리오 생성 알고리즘**
   - 무작위 탐색 (Randomized Search)
   - 유효한 확장 방향 탐색 (동/서/남/북/상)
   - N개 조합 생성 (예: 100개)

5. **Metrics & Plots**
   - 층별 기둥 설치 진행률
   - 누적 부재 수
   - xy plot 출력

6. **재생 제어**
   - play, pause
   - seek bar (특정 step 이동)
   - 배속 재생

---

## Work Objectives

### Must Have

- Simulation Mode 활성화
- Grid 환경 설정 UI
- Workfront 선택 (Grid 교차점)
- 시나리오 생성 (최소 1개 알고리즘)
- Step 시뮬레이션 실행
- Metrics 계산
- 재생 제어 UI

### Must NOT Have

- Time & Cost 산정 (범위 외)
- Duration 계산 (범위 외)

---

## 기술 스택

| 항목 | 선택 |
|------|------|
| **Python** | 3.12 + pandas (1, 2단계와 동일) |
| **Rust 그래픽** | eframe (1, 2단계와 동일) |
| **추가 의존성** | matplotlib (Python plot, 선택사항) |

---

## 주요 태스크 (개요)

1. Simulation Mode UI 활성화
2. Grid 환경 설정 UI 구현
3. Grid 기반 Node/Element 자동 생성
4. Workfront 선택 UI (Grid 교차점 클릭)
5. 시나리오 생성 알고리즘 구현
6. Step 시뮬레이션 실행 로직
7. Metrics 계산 로직
8. Plot 출력 (층별 진행률, 누적 부재)
9. 재생 제어 UI (play/pause/seek/배속)
10. 통합 테스트

---

## 환경 설정 파라미터

```python
# Grid Line 설정
x_grid_count: int      # x 방향 Grid Line 갯수
y_grid_count: int      # y 방향 Grid Line 갯수
z_levels: int          # z 레벨 갯수 (층수)
z_interval: float      # 층고 (상수)

# 제약 조건
floor_column_constraint: float  # 0~1, 층별 기둥 설치율 제약
```

---

## 의존성

- **선행**: 개발 1, 2단계 완료
- **후행**: 없음 (최종 단계)
