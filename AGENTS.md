# AssyPlan Development Guidelines

## Development Environment

based on Windows 11 x64 platform

## Project Structure

### Skills Documentation Convention

각 개발 단계별 SKILLS.md 파일을 다음 경로에 저장:

```
.opencode/skills/
├── dev-phase1/SKILLS.md    # 개발 1단계 (완료)
├── dev-phase2/SKILLS.md    # 개발 2단계 (예정)
├── dev-phase3/SKILLS.md    # 개발 3단계 (예정)
└── ...
```

**규칙**:
1. 각 개발 단계 완료 시 해당 단계의 SKILLS.md를 `.opencode/skills/dev-phase{N}/` 경로에 저장
2. SKILLS.md에는 해당 단계에서 사용한 기술 스택, 아키텍처, 구현 패턴 문서화
3. 프로젝트 루트의 SKILLS.md는 현재 진행 중인 단계의 내용 유지

### Virtual Environment

- 경로: `.venv/` (venv 아님)
- 활성화: `.\.venv\Scripts\activate` (Windows)
- 의존성: `requirements.txt`

### Development Plans

개발 계획 문서 위치:
```
.sisyphus/plans/
├── dev-phase1.md    # 개발 1단계 계획 (완료)
├── dev-phase2.md    # 개발 2단계 계획 (개요)
├── dev-phase3.md    # 개발 3단계 계획 (개요)
```

## Phase Status

| Phase | 상태 | 설명 |
|-------|------|------|
| Phase 1 | ✅ 완료 | Development Mode - CSV 파싱, 검증, 3D 뷰어 |
| Phase 2 | ⏳ 예정 | Simulation Mode |
| Phase 3 | ⏳ 예정 | TBD |

## Key Technical Decisions

### Phase 1 (Development Mode)
- **Python**: 3.12+, pandas, numpy, charset_normalizer
- **Rust**: eframe 0.27 (egui + wgpu), PyO3 0.21
- **인코딩**: charset_normalizer로 UTF-8/EUC-KR 자동 감지
- **Node ID**: 1부터 시작, x→y→z 정렬
- **Element 분류**: Column (수직), Girder (수평)
- **Simulation Mode**: 버튼만 표시, 비활성 상태 유지
