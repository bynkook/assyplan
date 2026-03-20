# 프로젝트 개발계획서

## 프로젝트의 목표

본 프로젝트의 목표는 건축/토목 철골 구조물의 3D 형상 데이터를 기반으로, 물리적 적합성과 안정성을 만족하는 시공 조립 순서(Sequence)를 자동으로 생성하고, 다중 Workfront 환경에서의 시공 진행 상태를 3D 그래픽 및 데이터(Metrics)로 시뮬레이션하는 시스템을 개발하는 것입니다. 이를 통해 효율적인 시공 계획 수립과 사전 검증을 지원합니다.

## 개발 환경

Windows 11 x64 를 로컬 개발환경 기준으로 한다.

1. Python + Rust 를 기본으로 한다.
2. 데이터의 입출력은 사용자에게 익숙한 Python 으로 구현한다.
3. Rust 는 고성능 계산이 필요한 함수와 그래픽 인터페이스 개발에 사용한다.
4. Python 에서 데이터를 가공한 후 PyO3 바인딩을 통해 Rust 코어 엔진의 함수를 호출하여 메모리를 공유한다.
5. 로컬 파일(csv, json, pickle dump) 는 필요한 경우로 영구 저장 또는 비동기 통신용으로 사용할 수 있다. 또한 사용자의 디버깅이나 최종 결과 확인용으로 사용된다.

## 프로젝트의 기본 구조

1. **개발 모드** 는 사용자가 업로드 하는 csv 파일 데이터로부터 개발 1,2 단계를 진행하는데 결과물 확인용으로 사용하며 개발 완료 후에는 최종적으로는 사용자가 입력한 데이터의 간단한 시각화 툴로 사용된다.
2. **시뮬레이션 모드** 는 이 프로젝트의 최종 목표로 조립 순서를 설정된 작업 공간 과 환경 설정을 바탕으로 부재 조립 순서와 `step` 을 자동으로 생성하는 것이다.

## 프로젝트 개발 순서

### 개발 1단계

#### 개발 1단계 목표

- 사용자가 업로드한 데이터(보통 csv 형식의 텍스트 파일)를 읽고 node table, element table 을 생성하여 저장한다.
- 3D 그래픽 인터페이스 **개발 모드(Development Mode)** 를 구현한다.

#### 데이터 입력 및 출력

- Read data : 부재 id, 부재의 node_i (x, y, z), node_j (x, y, z)
- write data : node table(node id, (x, y, z)좌표), element table(element id, node_i, node_j, member_type) *※ member_type: Column, Girder 등 부재 종류 구분*
- note id, element id 의 번호는 정수(integer)이며 정렬 순서는 Catesian 좌표계 origin(0,0,0) 을 기준으로 z축 -> x축 -> y축 우선순위로 정렬하며 각 축의 positive direction 으로 번호가 증가한다.
- id 로 사용되는 integer 는 1부터 시작한다. 0은 사용하지 않는다.
- 데이터의 저장은 사용자의 검토와 편의를 위해 csv, json 등의 포멧된 텍스트 형식을 사용하여 저장한다. 그러나 실제 입출력의 속도를 최대한으로 향상하기 위해 실제 프로그램의 동작에 필요한 저장과 로드는 pickle 을 사용하여 바이너리 스트림으로 구현한다.

#### 입력데이터의 검사 : 부재(member)를 생성하는 가장 기본적인 원칙

- 부재의 길이가 x, y, z 축과 평행한 인접에 있는 절점으로 정의되는지 여부
- 임의의 2개의 부재의 설치 방향을 정의하는 vector의 cosine similarity는 0 또는 1 이다. 즉, x,y,z 축과 어느 한 방향으로 평행하게 배치된다.
- 수평 부재의 노드 i 와 노드 j 는 x 또는 y 값 둘 중 하나만 변화하며 x, y 두 값이 동시에 변화하지 않는다(즉, 그리드 라인과 평행하지 않은 대각선 부재는 없다)
- 수직 부재의 노드 i 와 노드 j 의 (x, y) 좌표는 같고 z 좌표가 동일하다
- z 좌표의 최소값은 모두 동일한 값이어야 한다. 즉, 1층 기둥은 모두 같은 바닥층 레벨에 설치된다고 설정한다.
- 예외(오류) 케이스의 검출 기능 구현

1. 노드(절점) 좌표가 어떤 element 와도 연결되지 않은 orphan 상태인지 아닌지 검사하는 로직을 구현해야 한다.
2. 동일한 ID 를 가진 node, element 의 검출
3. 부재 i 와 j 절점이 완전히 동일한 좌표로 설정되어 길이가 0 인 부재의 검출
4. 완전히 동일한 공간에 겹쳐서 배치된 중복 부재의 검출

#### 그래픽 엔진 및 입출력 인터페이스 개발(3D 그래픽)

- 상용 구조해석 프로그램의 pre/post 기능중 일부를 유사하게 구현한다.
- 개발 1단계는 사용자 데이터 업로드 기능을 **개발 모드(Development Mode)** 를 구현한다.
- 다음 개발 단계에서는 **시뮬레이션 모드(Simulation Mode)** 를 구현하므로 이 단계에서는 기능 전환 버튼만 미리 구현해두고 실제 기능은 동작하지 않는다.
- Lauout design : 창 상단부 Headings & Functions 영역은 주로 기능 버튼들을 배치, 그 아래 메인화면은 수직 2분할 하여 왼쪽은 상태 표시창, 오른쪽은 그래픽 view window & view controls 를 구현한다.
- 오른쪽 그래픽 view window 에는 환경설정 탭, view 탭, result 탭을 구현하며 사용자는 마우스 클릭으로 탭을 전환할 수 있다.
- view window 내부에는 view axis 를 3D cubic face 로 구현.
- view window 내부에서 사용자는 x-y plan, y-z plan, z-x plan 뷰 전환, 3D orbit 뷰 회전, zoom, pan 기능 작동 가능
- grid lines의 생성 : 입력데이터에서 모든 x, y 좌표의 z = min(z) 레벨로의 투영(projection). x-y plan view 에서 사용자는 절점값 입력에 오류가 없는지 grid line 의 간격과 위치로부터 판단할 수 있다.
- node, element 출력 : node 는 매우 작은 dot 으로 표시. element 는 직선으로 표시
- node_id, element_id view on/off 기능 구현. 폰트 크기는 view window 사이즈와 zoom level 에 따라 자동 조정되어 사용자에게 항상 일정한 글자 크기로 보여줘야 한다.
- 입력파일 업로드 완료한 상태에서 `recalc` 버튼을 누르면 데이터 검사를 수행 -> 검사결과 출력 -> 그래픽 출력 단계로 이동한다.
- 입력파일 업로드 완료한 상태에서 `reset` 버튼을 누르면 이전의 데이터 검사와 화면 출력은 모두 초기화 된다.

#### 입력데이터 컬럼명과 의미

1. 부재_ID : 부재의 고유 ID
2. node_i_x, node_i_y, node_i_z : 노드(절점) i end 의 (x,y,z) 좌표
3. node_j_x, node_j_y, node_j_z : 노드(절점) j end 의 (x,y,z) 좌표
4. 선행부재_ID : 바로 앞 순서에 설치된 부재의 고유 ID

#### 개발 1단계에 필요한 SKILLS.md 작성

개발 1단계의 작업 내용을 바탕으로 개발에 필요한 SKILL 을 정의하여 프로젝트 폴더에 저장한다.

### 개발 2단계

#### 개발 2단계 목표

**개발 모드(Development Mode)** 의 그래픽 인터페이스에 시공단계를 사용자에게 보여주는 기능을 추가한다.

#### 개발 2단계의 구현해야 할 기능

- 사용자가 입력한 전체 모델에 *member assemble sequence 의 적합 및 안정 조건(compatibility and stability condition)* 을 만족하지 않는 부재가 있는지 검사
- 사용자가 입력한 데이터의 각 부재별 설치 순서를 정의한 *선행 부재* 에 따라 부재 생성 순서를 정렬하여 정리한 *construction sequence table* 을 workfront 별로 생성하고 저장한다.
- *member assemble sequence 의 적합 및 안정 조건(compatibility and stability condition)* 을 만족하는 단위 step 을 workfront 별로 생성하는 기능
- workfront 별로 생성된 step 을 오름차순으로 정렬하여 *workfront step table* 을 생성하고 저장.
- step을 누적하여 생성된 모델의 현재 상태의 전체를 대상으로 *member assemble sequence 의 적합 및 안정 조건(compatibility and stability condition)* 을 만족하는지 검사하는 기능(double check)
- 이미 적합조건을 통과한 부재들은 반복 검토 루프에서 제외하여 검사 목적의 계산속도를 높이는 로직
- 현재 설치된 구조물의 상태와 공사 진척도를 표현하는 `metrics & measures` 를 계산하는 기능

#### 개발 2단계 기능 구현 참고사항

- *construction sequence table* 의 데이터 컬럼은 workfront_id(integer), member_id(integer) 로 정의되며 부재 생성 순서대로 member_id 가 순서대로 기록된다.
- *workfront step table* 의 데이터 컬럼은 workfront_id(integer), step(integer), member_id(integer) 로 정의.
- *workfront step table* 에는 step 숫자가 동일한 여러개의 member_id 가 존재할 수 있다.
- integer 는 1부터 시작하는 정수. 0은 사용하지 않는다.
- **개발 모드(Development Mode)** 의 그래픽 인터페이스에 시공 단계를 추가하는 기능 구현은 step 1 부터 step 최대값 까지 화살표를 클릭하여 시공단계를 한단계씩 view window 를 갱신하며 출력하는 기능을 포함한다. step 숫자를 입력하여 임의의 step 으로 이동하는 기능도 구현.
- 그래픽 출력은 이전 출력의 상태를 미리 그려논 상태에서 추가 요소를 생성하거나 삭제하는 방법으로 출력 속도를 최대한으로 향상시키는 고품질 캐시(cache) 전략이 필요하다. 1만개의 부재를 가진 모델 랜더링시 60fps 유지.
- 입력파일 업로드 완료한 상태에서 `recalc` 버튼을 누르면 데이터 검사를 수행 -> 검사결과 출력 -> 그래픽 출력 단계로 이동한다.
- 입력파일 업로드 완료한 상태에서 `reset` 버튼을 누르면 이전의 데이터 검사와 화면 출력은 모두 초기화 된다.

#### 개발 2단계에 필요한 SKILLS.md 작성

개발 1, 2단계의 작업 내용을 바탕으로 개발에 필요한 SKILL 을 정의하여 프로젝트 폴더에 저장한다.

### 개발 3단계

#### 개발 3단계 목표

- **시뮬레이션 모드(Simulation Mode)** 의 구현
- 환경 설정된 grid 범위 내 모든 기둥과 거더 부재를 생성하기 위한 node table, element table 을 생성하는 기능 구현
- 이 **부재 생성 단계** 기능은 시뮬레이션 중 반복적으로 발생할 수 있는 **기하학적·중복·유효성 오류**를 한 번에 검사하고 필터링할 수 있게 해준다. 따라서 실제 시뮬레이션 루프(부재 추가 단계)에서는 미리 확보한 부재 리스트에서 생성할 부재를 선택하게 되어 **검사 함수 호출 횟수를 획기적으로 줄이고**, **런타임 오류 발생 가능성을 거의 제거**할 수 있다.
- multiple workfront 시뮬레이션의 구현 : 사용자는 그래픽 화면에 그려진 grid 라인의 교차점을 선택하여 1개 이상의 workfront 를 설정한다.
- 각 Workfront 를 시작점으로 기둥 1개 설치하고 탐색 알고리즘을 실행하여 다음 부재 설치 위치를 결정한다.
- 유효한 시나리오를 생성하고 시뮬레이션 결과를 GUI 에 출력한다.
- 선택된(1개 또는 여러개) 시나리오의 Metric XY Plot 출력 : x축은 step, y축은 각 층별 부재수 합계 또는 진행율 등.
- `recalc` 버튼은 시뮬레이션 실행에 사용된다.
- 재생 제어 : play, pause, 특정 step으로 이동(seek bar), 배속 재생 기능
- 사용자 디버깅 및 검토용 텍스트 파일을 별도로 저장한다.

#### 시뮬레이션 모드의 핵심 탐색 원칙

대규모 그리드(60×20)에서 전수 탐색은 불가능하므로 Monte-Carlo 샘플링을 유지하되, **Step당 추가 부재 수를 최소화하고 기존 구조물에 가장 인접한 위치를 최우선**으로 하는 Minimal Incremental Attachment 전략을 기본 채택한다. 이는 실제 시공 현장에서 “한 번에 큰 독립 조립체를 만들기보다는 이미 안정된 부분에 바로 붙여가며 점진적으로 확장”하는 현실적인 순서를 반영한다.

이 전략에서 유지해야 하는 공통 원칙은 아래와 같다.

- Sequence 와 Step 은 서로 다른 개념으로 유지한다.
- Sequence 는 단위 시간이며, 각 Sequence 에서 각 workfront 는 원칙적으로 1개 부재만 설치한다.
- Step 은 개별 부재가 아니라 **패턴 기반 적합 안정 조건**을 통과한 부재 그룹이다.
- Step 은 부재 수가 일정 개수에 도달했다고 해서 강제로 완료되면 안 된다. **Step 완료는 오직 안정 조건 pass 기준**으로만 결정한다.
- Step당 생성 부재 수 목표 : **최소 2~3개**. 기둥 3개 + 거더 2개 독립 단위는 최후순위 bootstrap 으로 둔다.
- 기존 구조물과의 연결 개수 최대화, Workfront 프론티어로부터의 거리 최소화를 동시에 추구한다.
- 상층부 제약과 적합 안정 조건은 후보 생성 이후가 아니라 **부재 생성 시도 시점**부터 강제 검증한다.
- 타겟은 미설치 기둥이 남아 있는 workfront 와 가까운 절점이다.
- 아래층 기둥이 100% 설치 완료되면 상층부 기둥 설치는 더 이상 하층 진행율 제약의 직접적인 방해를 받아서는 안 된다.
- 어떤 층의 기둥 설치가 100% 완료되고 적합 안정조건을 만족하면, 탐색 범위의 중심은 하층부가 아니라 상층부로 이동한다.
- 기둥이 거의 다 설치된 상태에서는 거더 1~2개가 없어도 적합 안정조건을 만족할 수 있으므로, step과 잔여 미설치 부재를 같은 개념으로 취급하면 안 된다.

#### 시뮬레이션 엔진의 실행 구조

시뮬레이션 엔진은 Monte-Carlo 시나리오 샘플링을 기반으로 하되, 매 후보 생성마다 안정성 검사 + 상층부 제약을 강하게 pruning 하고, Minimal Incremental 점수(부재수 최소화 + 연결성 + 거리)로 weighted sampling 하여 incremental greedy-ish 방향을 선택한다. 이 조합으로 60×20 규모에서도 100개 시나리오를 1~2분 이내에 완료하는 것을 목표로 한다.

```Rust
// 전체 알고리즘 구조 예시 (한 눈에 보는 흐름)

// Rust 코어 내부 (매 step 호출)
fn generate_next_step(current_structure: &Structure, workfronts: &[Workfront]) -> Option<Step> {
    for wf in workfronts.iter() {
        // 1. 후보 생성 (incremental greedy-ish)
        let candidates = generate_incremental_candidates(wf, &current_structure);  // 1~3개 단위 위주

        // 2. 강한 pruning (개발 1단계 검사)
        let valid_candidates = candidates
            .into_iter()
            .filter(|c| validate_basic_geometry(c))
            .filter(|c| check_assembly_stability(c, current_structure))
            .filter(|c| !violates_upper_floor_constraint(c))
            .collect::<Vec<_>>();

        if valid_candidates.is_empty() { continue; }

        // 3. weighted sampling
        let scores = valid_candidates.iter().map(|c| compute_score(c)).collect();
        let chosen = weighted_random_choice(&valid_candidates, &scores);

        return Some(Step::new(wf.id, chosen));
    }
    None
}
```

현재 채택한 실행 구조는 Sequence-driven Monte-Carlo + 강한 Pruning + Workfront-local Weighted Sampling + Pattern-buffered Step 생성 조합이다. 이 구조의 핵심은 “어떤 Step 을 직접 한 번에 고른다”가 아니라, **각 Sequence 에서 각 workfront 가 1개 부재를 고르고, 그 결과를 workfront별 패턴 버퍼에 누적한 뒤, 완성 패턴일 때만 Step 으로 방출한다** 는 점이다.

##### 1. Monte-Carlo (Randomized Scenario Sampling)

- 역할 : 100~200개의 서로 다른 실현 가능한 조립 순서를 빠르게 샘플링한다.
- 각 시나리오는 독립 random seed 를 사용한다.
- rayon 기반 병렬 실행을 기본으로 한다.
- 시나리오 간에는 상태를 공유하지 않는다.

##### 2. Sequence-driven parallel installation

- Sequence 는 단위 시간이다.
- 각 Sequence 에서 각 workfront 는 **최대 1개 부재** 만 설치 후보로 선택한다.
- workfront 가 N개면 초기에는 보통 N개 부재가 동시에 추가되고, 종료 직전에는 1개, 마지막에는 0개가 되어 종료된다.
- 같은 Sequence 번호는 같은 시간 라운드를 뜻하므로, 여러 workfront 가 같은 Sequence 에서 동시에 설치되면 **동일한 sequence 번호를 공유**한다.
- 따라서 같은 Sequence 안에서 workfront A 는 1층, workfront B 는 2층처럼 **서로 다른 층에서 동시에 작업**할 수 있다. 단, 개별 workfront 는 그 Sequence 에서 1개 층에서만 1개 부재를 설치한다.
- 같은 Sequence 내에서 동일 부재를 두 workfront 가 동시에 선택하면 안 된다.
- Step 은 Sequence 와 별개이며, Sequence 1회마다 Step 1개를 기계적으로 만들지 않는다.

##### 3. Global Step Cycle aggregation

- 시뮬레이션 루프는 `global step cycle` 단위로 동작한다.
- cycle 내부에서 각 workfront 는 sequence round 마다 최대 1개 부재만 선택한다.
- workfront 로컬 버퍼가 완성 패턴 + 안정 조건 pass 를 만족하면 `LocalStep` 으로 cycle 수집 버퍼에 저장한다.
- 같은 cycle 에서 local step 생성에 성공한 workfront 는 해당 cycle 남은 라운드에서 제외된다.
- cycle 이 종료되면 수집된 여러 `LocalStep` 을 1개의 `SimStep` 으로 병합한다.
- 병합된 global step 의 `element_ids` 는 local step 들의 union 이며, 디버깅/표시를 위해 `local_steps` 원본 구조를 유지한다.
- sequence 번호는 1부터 시작하는 global 연속 번호를 사용한다.
- 각 workfront 의 sequence 는 승인된 local step 의 `element_ids` 를 생성 순서대로 이어붙인 WF 연속 이력이다.
- Sequence 뷰는 cycle(step) 순서 기반으로 정렬한다. 각 cycle 블록 내에서 WF별 원소를 position interleave하여 전역 sequence 를 구성하고 누적으로 표시한다. 이 방식은 지지 기둥이 반드시 거더보다 먼저 표시됨을 보장한다.
- Step 뷰는 안정성 패턴 단위 뷰이며, Sequence 뷰와 의미를 섞지 않는다.

##### 4. Workfront-local candidate search

- 각 workfront 는 자신의 `(x, y)` 시작점과 현재 buffer 가 형성한 로컬 footprint 근처에서만 다음 후보를 탐색한다.
- 후보 위치는 전역 frontier 전체보다 **해당 workfront 인접성** 을 우선한다.
- 여러 workfront 를 지정하면 각 workfront 주변에서 부재 생성이 병렬적으로 진행되는 모습이 보여야 한다.
- 후보 탐색은 round 시작 시 계산한 `allowed_floors` 와 `committed_floor` 제약 안에서만 수행한다.
- 구현은 “후보 묶음을 한 번에 만든 뒤 나중에 부분집합을 추출”하는 방식이 아니라, **후보를 하나씩 시도하고 현재 local step 을 진전시키는 후보만 채택하는 retry loop** 여야 한다.

##### 5. 강한 Pruning (개발 1·2단계 검사 로직 재사용)

- 매 후보 생성 직후 기하학 및 설치 가능 조건을 먼저 검사한다.
- 그 다음 floor gate 와 Step 적합 안정 조건을 순차적으로 검사한다.
- Step 적합 안정 조건 검사는 매번 모델 전체를 다시 훑지 않고, **후보 패턴과 실제로 맞닿는 국소 안정 문맥(local stable context)** 만 추출하여 수행한다.
- 단, 국소 안정 문맥이 비어 있다고 해서 곧바로 fail 하지 않는다. 이 경우에는 **독립 bootstrap 가능성** 을 별도로 검사한다.
- 현재 구현은 아래 순서의 구조적 판정을 사용한다.

1. 미리 생성된 유효 element pool 에서만 후보를 고른다.
2. `allowed_floors` 와 `committed_floor` 제약으로 floor 를 먼저 줄인다.
3. 로컬 footprint 와 anchor 기준 locality 조건을 통과한 후보만 남긴다.
4. 현재 buffer 시그니처가 요구하는 `StepCandidateMask` 와 맞는 후보만 남긴다.
5. 후보를 하나 선택해 현재 buffer 를 실제로 진전시키는지 검사한다.
6. complete pattern 이 되면 `check_step_bundle_stability` 로 최종 적합 안정 판정을 한다.

##### 6. Weighted Sampling – Minimal Incremental Attachment Search

- 각 workfront 는 다음 확장 후보를 생성할 때 **완성 가능성이 있는 증분 패턴 방향** 을 우선 고려한다.
- 우선 후보 순서는 아래와 같다.

1. 1기둥 + 1거더
2. 1기둥 + 2거더
3. 2기둥 + 1거더
4. 2기둥 + 2거더
5. 독립 3기둥 + 2거더 bootstrap

- 기존 구조에 연결 가능한 증분 후보가 1개라도 있으면 독립 bootstrap 후보보다 우선한다.
- bootstrap bundle 은 `w1/w2/w3` 점수로 weighted sampling 한다.
- bootstrap 이후 증분 확장은 **single-candidate retry loop** 로 수행한다. 즉, 현재 round 에서 선택된 후보가 local step 을 진전시키지 못하면 즉시 버리고 다음 후보를 다시 뽑는다.
- 현재 코드에서 incremental 점수는 `w1`(connectivity), `w2`(frontier distance) 기준으로 계산되고, `w3` 는 bootstrap bundle 점수에 반영된다.

```rust
fn compute_score(c: &Candidate) -> f64 {
  0.50 * (1.0 / c.member_count as f64)
    + 0.30 * c.connectivity_score()
    + 0.15 * (1.0 / c.frontier_distance())
    + 0.05 * (if c.is_lowest_floor() { 1.0 } else { 0.0 })
}
```

##### 7. Pattern-buffered Step generation

- 각 workfront 는 자신의 Sequence 결과를 별도 버퍼에 누적한다.
- 버퍼가 sub pattern 상태이면 Step 을 생성하지 않고 다음 Sequence 로 넘긴다.
- 버퍼가 완성 패턴이 되고 적합 안정 조건을 pass 할 때만 Step 을 방출한다.
- buffer 분류는 `Incomplete(mask)`, `Complete(pattern)`, `Invalid` 3가지로만 다룬다.
- `Incomplete(mask)` 는 다음 후보의 member type 을 제한하는 구조적 규칙이며, 임시 점수 보정이나 휴리스틱 우회 규칙이 아니다.
- `Invalid` 또는 `Infeasible` 이 된 buffer 는 즉시 rollback 한다.
- Step 은 부재 개수만으로 강제 완료하지 않는다.
- 정상 결과에서는 Step 수가 Sequence 수보다 현저히 적어야 한다.

##### 8. Early termination

- 종료 조건은 “더 이상 유효 후보가 없음”, “장기간 진전이 없음”, “모든 부재 설치 완료” 같이 시나리오 수준의 조건으로 제한한다.
- 부재 수 또는 Step 개수만으로 종료하지 않는다.
- 독립 5개 단위의 과다 사용, 상층부 위반 반복, 장기간 무진전은 실패 원인 메타데이터로 기록할 수 있다.


#### 시뮬레이션 모드(Simulation Mode) 의 환경설정, 제약, 메뉴 구성

시뮬레이션 모드를 시작하면 환경설정에서 설정한 Grid Plane 을 표시한 x-y plan view 가 먼저 표시되고, 사용자는 그 교차점을 선택하여 1개 이상의 workfront 를 지정한다. 이 환경설정은 단순 UI 옵션이 아니라, 실제 시뮬레이션 후보 생성 범위와 층별 제약 동작을 결정하는 입력값이다.

##### Grid Line 환경 설정

1. x, y 방향 Grid Line 갯수
2. grid xy plane 갯수 (`z=0` 이 첫번째 grid plane)
3. z 레벨 간격 : 레벨간 층고(일단 이번 연구에서는 상수값 고정)

##### Workfront 선택 기능

- x-y plan view 에서 사용자가 grid 교차점을 선택하여 workfront 위치를 지정한다.
- 여러 workfront 를 동시에 선택할 수 있어야 한다.

##### 제약 파라미터 설정

###### 1. 상층부 기둥 설치율 제약 (`upper floor column rate threshold`)

- 임의의 층 `N`에 대해 `(N+1층 기둥 누적 설치 갯수) / (N층 기둥 누적 설치 갯수)` 비율이 임계값(threshold)을 초과하지 않도록 제어한다.
- 사용자는 `0~1` 사이의 값을 설정할 수 있으며 기본값은 **0.3(30%)** 이다.
- 이 값이 낮을수록 상층부 공사는 하층부가 충분히 완료된 후에만 진행할 수 있다.
- `N`층 기둥이 1개도 설치되지 않은 경우(분모=0) 비율은 `0.0`으로 처리한다.
- 시뮬레이션 모드에서는 이 값은 참고용이 아니라 **hard block** 으로 사용된다.

###### 2. 하층부 기둥 완료율 기준 (`lower floor column completion ratio threshold`)

- 임의의 상층 후보에 대해 하층 기둥 완료율 `하층 설치 기둥 / 하층 전체 기둥`이 임계값 이상일 때만 상층 신규 탐색을 허용한다.
- 사용자는 `0~1` 사이의 값을 설정할 수 있으며 기본값은 **0.5(50%)** 이다.
- 이 값은 상층부 진입 시점을 제어하는 **hard gate** 이다.

###### 3. 추가 제약 : 증분 후보 우선

- 하나의 Step 에서 독립안정구조(5개 단위)를 생성할 수 있는 경우라도, 기존 구조에 연결 가능한 증분 후보가 1개 이상 존재하면 반드시 증분 후보를 우선 선택하도록 강제한다.
- 즉, bootstrap 은 가능하더라도 항상 1순위 선택이 아니다.

###### 4. Floor Commitment 제약

- workfront 가 어떤 층에서 step 형성을 위해 첫 부재를 buffer 에 넣는 순간, 해당 층에 commitment 된다.
- commitment 상태에서는 동일 층 후보만 계속 탐색하며 step 완성(Complete + 안정 조건 pass) 전에는 다른 층으로 이동하지 않는다.
- commitment 상태에서 유효 후보가 완전히 소진되면 즉시 rollback 한다.
- rollback 시 해당 buffer 부재를 workfront 로컬 점유에서 해제하고, `buffer_sequences`, `committed_floor` 를 초기화한 뒤 다음 라운드에서 다시 제약조건 기반 탐색으로 복귀한다.
- step 이 완성되어 LocalStep 이 방출되면 commitment 는 해제된다.

###### 5. 2-threshold + commitment 정책의 결합 규칙

- 상층부 기둥 설치 후보는 먼저 `upper floor column rate threshold`를 검사한다. `(상층 누적 + 1) / 하층 누적 > threshold` 이면 후보를 거부한다.
- workfront 가 아직 uncommitted 인 상태에서 상층 후보를 신규 탐색하려면 아래 Gate 를 모두 통과해야 한다.
- 하층 기둥 완료율 `하층 설치 기둥 / 하층 전체 기둥 >= lower floor column completion ratio threshold`
- 즉, 현재 구현에서 hard constraint 는 `upper floor column rate threshold` 와 `lower floor column completion ratio threshold` 두 가지이며 bonus 점수 가감은 사용하지 않는다.

#### 성능 목표 및 최적화 전략

- 목표 모델 규모 : 최대 60×20 그리드 ≈ 12,000~25,000 부재
- 1개의 시나리오 평균 소요시간 : **0.7~1.8초** (Minimal 전략은 후보 수가 적어 더 빨라짐)
- 100개 시나리오 전체 소요시간 : **1.2~3분** 이내 (8코어 병렬)


#### 시각화 및 사용자 인터페이스

- 선택된(1개 또는 여러개) 시나리오의 Metric XY Plot 을 출력한다. x축은 step, y축은 각 층별 부재수 합계 또는 진행율 등이다.
- Metric XY Plot에 **Step당 평균 추가 부재 수** 추세선을 추가한다.
- 시나리오 비교 테이블에 “평균 연결성 점수”, “평균 Step 부재 수” 컬럼을 추가한다.
- 실패 원인 표시에 “독립 5개 단위 과다 사용” 항목을 신설한다.
- `recalc` 버튼은 시뮬레이션 실행에 사용된다.
- 재생 제어는 play, pause, 특정 step으로 이동(seek bar), 배속 재생 기능을 포함한다.
- trace logger 는 text 로그를 기본으로 하며, 필요 시 JSONL 동시 저장 옵션을 제공한다.

#### 개발 3단계에 필요한 SKILLS.md 업데이트 항목

- Rust rayon을 활용한 시나리오 병렬 실행 구현
- single-candidate retry loop 와 rollback 구조
- buffer classification (`StepBufferDecision`, `StepCandidateMask`) 반영
- complete pattern emission / infeasible rollback 분리
- weighted random choice 알고리즘
- 상층부 제약 검사 최적화 (층별 기둥 카운트 캐시)
- 사용자 가중치 슬라이더 UI 연동

## 용어 및 Step 적합 안정 조건 정본

이 절은 기존 STABILITY_ANALYSIS.md 를 devplandoc.md 로 통합한 최종 정본이다. Step 적합 안정 조건, 용어 정의, 국소 안정 문맥 기반 판정 규칙은 이 절을 기준으로 유지한다.

### 용어 정리

- Step: 패턴 기반 적합 안정 조건을 통과한 부재 그룹이다. 개별 부재 저장 단위가 아니며, count-based 규칙으로 강제 완료하지 않는다.
- Sequence: 단위 시간 라운드다. 각 workfront 는 같은 Sequence 에서 최대 1개 부재만 설치하며, 같은 라운드 동시 설치는 동일 sequence 번호를 공유한다.
- 시공단계: step 과 같은 의미로 사용한다.
- Workfront: 작업조의 주된 평면 위치다. 모든 workfront 는 동시에 시작하며, 개별 workfront 는 한 Sequence 에서 한 층에서만 1개 부재를 설치할 수 있다.
- Node: grid line 교차점을 의미하는 (x, y, z) 좌표점이다.
- Member: 두 노드로 정의되는 구조 부재다.
- Column: node_i 와 node_j 의 (x, y) 가 같고 z 만 다른 수직 부재다.
- Girder: node_i 와 node_j 의 z 가 같고 x 또는 y 축 방향으로만 놓이는 수평 부재다.
- 선행부재: 부재 생성 순서의 전후 관계를 정의하는 입력 데이터다.
- Floor: 1층은 첫 번째 기둥이 설치되는 레벨, 2층은 1층 기둥 node_j 레벨이다.

### 입력 데이터와 부재 정의 기본 원칙

- 부재는 x, y, z 축과 평행한 인접 절점을 연결해야 한다.
- 임의의 2개 부재 설치 방향 vector 의 cosine similarity 는 0 또는 1 이어야 한다.
- 수평 부재는 x 또는 y 중 하나만 변화해야 하며 대각선 부재는 허용하지 않는다.
- 수직 부재는 node_i 와 node_j 의 (x, y) 좌표가 같고 z 좌표만 달라야 한다.
- 바닥층 최소 z 좌표는 전체 입력에서 동일해야 한다.
- 오류 검출 대상은 orphan node, 중복 ID, 길이 0 부재, 중복 부재다.

### 핵심 구분

- 설치 가능 후보 판정은 개별 부재 기준이다.
- Step 적합 안정 조건 판정은 반드시 패턴 기준이다.
- 기둥 node_i 가 하부 기둥 node_j 에 적층된다는 사실은 설치 가능 조건이지, step 안정 판정 근거 자체는 아니다.
- 순수 수직 적층(column stacking) 만으로는 인접 안정 구조와의 횡방향 연결로 보지 않는다.
- Step 수는 Sequence 수보다 현저히 적어야 정상이며, Step 수가 Sequence 수와 1:1에 가까우면 패턴 판정 로직 회귀로 본다.

### 부재 생성 및 지지 조건

- 기둥과 거더는 반드시 grid line 교차점 노드 사이에서만 생성한다.
- 부재 생성 방향은 동, 서, 남, 북, 상 방향만 허용하며 grid space 를 벗어나면 안 된다.
- z=0 레벨의 거더는 허용하지 않는다.
- 이미 안정된 구조물에 연결되는 step 에서는 새로 추가되는 모든 부재가 각각의 지지 조건을 만족해야 한다.
- 기둥 node_i 는 바닥층 또는 이미 안정 판정을 받은 하부 기둥 node_j 에 연결되어야 한다.
- 거더는 양쪽 노드가 모두 안정 판정을 받은 기둥에 지지되어야 하며 캔틸레버는 불허한다.

### Step 적합 안정 조건 요약

#### Bootstrap

- 첫 독립 step 은 `기둥→기둥→거더→기둥→거더` 패턴으로 본다.
- 지상층 기둥 3개와 90도 직교 거더 2개가 필요하다.
- 국소 안정 문맥이 비어 있을 때는 bootstrap 최소 조립체 여부로만 pass 가능하다.
- 평행한 2개 거더로 연결된 3기둥 구조는 안정 구조로 인정하지 않는다.

#### 1개 부재 패턴

- `(거더)`: 이미 설치된 두 기둥을 연결하는 폐합 부재일 때만 pass.
- `(기둥)`: 단독으로는 step 이 될 수 없다.

#### 2개 부재 패턴

- `(기둥→기둥)`: sub pattern 이며 단독 step 불가.
- `(기둥→거더)`: 인접 안정 구조의 기둥 node_j 와 거더로 연결될 때 pass.
- `(거더→거더)`: sub pattern 이며 단독 step 불가.

#### 3개 부재 패턴

- `(기둥→기둥→기둥)`: 절대 불허.
- `(기둥→기둥→거더)`: sub pattern.
- `(기둥→거더→기둥)`: sub pattern.
- `(기둥→거더→거더)`: 두 거더 모두 기둥에 지지되고 캔틸레버가 아닐 때만 pass.

#### 4개 부재 패턴

- `(기둥→기둥→거더→거더)`: 인접 구조물과 연결될 때 pass.
- `(기둥→기둥→기둥→거더)`: 절대 불허.
- `(기둥→거더→기둥→거더)`: 인접 구조물과 연결될 때 pass.

#### 5개 부재 패턴

- `(기둥→기둥→거더→기둥→거더)`: 독립 존재 가능하며 bootstrap 으로 허용한다.

### 금지 패턴과 Sub Pattern 규칙

- `기둥→기둥→기둥` 은 어떤 경우에도 step 으로 인정하지 않는다.
- `기둥→기둥→기둥→거더` 는 어떤 경우에도 step 으로 인정하지 않는다.
- `독립 기둥 2개 + 거더 1개` 상태는 step 으로 인정하지 않는다.
- 캔틸레버 거더는 step 으로 인정하지 않는다.
- 독립된 1개 기둥은 개별 step 으로 저장하지 않는다.
- sub pattern 은 임시 버퍼 상태이며 상위 완성 패턴이 될 때까지 step 을 생성하지 않는다.

### 국소 안정 문맥 기반 판정 플로우

1. workfront 확장 후보를 생성한다.
2. 후보가 현재 buffer 를 실제로 진전시키는지 검사한다.
3. 후보 패턴 endpoint 와 닿는 국소 안정 문맥을 추출한다.
4. 국소 안정 문맥이 있으면 확장 패턴으로 평가한다.
5. 국소 안정 문맥이 없으면 독립 bootstrap 가능성으로 평가한다.
6. 금지 패턴, bundle connectivity, 지지 조건, 연결성, 캔틸레버 여부를 순차 검사한다.
7. 완성 패턴 + 적합 안정 조건 pass 일 때만 LocalStep 을 생성한다.

### 층별 우선 완료 규칙

- 현재 canonical 구현은 별도 `forced completion threshold` 를 두지 않는다.
- 하층 우선성은 `upper floor column rate threshold` 와 `lower floor column completion ratio threshold` 의 hard gate 로만 제어한다.
- 목적은 상층부 진입 타이밍을 구조적으로 제한하되, 예외용 보정 로직 없이 단순한 제약 기반 탐색을 유지하는 것이다.

### 현재 구현 기준 반영 사항

- Sequence-driven parallel installation 을 사용한다.
- 각 workfront 는 같은 Sequence 라운드에서 최대 1개 부재만 선택한다.
- 각 round 의 active workfront 수는 남은 미설치 부재 수에 비례해 줄어든다.
- workfront 별 버퍼에 Sequence 결과를 누적하고 완성 패턴일 때만 LocalStep 을 만든다.
- Step 생성은 workfront 즉시 방출이 아니라 global step cycle 집계 방식이다.
- 같은 cycle 에서 LocalStep 생성에 성공한 workfront 는 해당 cycle 의 남은 라운드에서 제외된다.
- cycle 종료 시 여러 LocalStep 을 1개의 global step 으로 병합한다.
- 각 workfront 의 sequence 는 승인된 local step 의 `element_ids` 를 생성 순서대로 이어붙인 WF 연속 이력이다.
- Sequence 뷰는 cycle(step) 순서 기반으로 정렬한다. 각 cycle 블록 내에서 WF별 원소를 position interleave하여 전역 sequence 를 구성하고 누적으로 표시한다. 이 방식은 지지 기둥이 반드시 거더보다 먼저 표시됨을 보장한다.
- invalid / infeasible / no-candidate committed buffer 는 즉시 rollback 한다.
- trace logger 는 승인된 local step 이력을 workfront 별로 기록할 수 있다.
- 결과적으로 `Sequence != Step` 가정이 유지되어야 하며 multi-workfront 에서 Step 수는 Sequence 수보다 작아야 정상이다.

### 구현 원칙 요약

- 전역 재검사를 기본 전략으로 사용하지 않는다.
- 신규 부재만 떼어 검사하는 방식도 사용하지 않는다.
- 신규 패턴 + 실제 인접 stable 부재만 합친 국소 문맥 평가를 기본으로 한다.
- 국소 문맥이 비어 있으면 독립 bootstrap 규칙으로 판정한다.
- 완성 패턴 + 적합 안정 조건 pass 일 때만 LocalStep 을 만들고 cycle 종료 시 global step 으로 병합한다.
- sequence 는 global 1-based 연속 번호를 유지하고 같은 round 동시 설치는 동일 번호를 공유한다.

## Metrics & Plots

이 절은 차트 출력용 데이터와 사용자에게 보여줄 지표 정의만 다룬다. 제약조건, threshold 기준, floor commitment 정책은 모두 **개발 3단계 > 시뮬레이션 모드의 환경설정, 제약, 메뉴 구성** 절에서 정의한다.

### 설치 진행 지표

- *층별 기둥 설치율* : 각 층별 해당 Step 에서 모든 Workfront 로부터 설치된 `(모든 기둥 갯수의 합계) / (층별 전체 기둥 갯수)`
- *상층부 기둥 설치율 차트 (Upper-Floor Column Installation Rate Plot)* : 임의의 층 `N`에 대해 **(N+1층 기둥 누적 설치 갯수) / (N층 기둥 누적 설치 갯수)** 비율을 계산하여 차트에 표시한다.
- *레전드 표기* : `F{N+1}/F{N}` (예: F2층 열은 `2층 설치수 / 1층 설치수`)
- *분모=0 처리* : `N`층 기둥이 1개도 설치되지 않은 경우 비율은 **0.0** 으로 표시한다.
- *개발 모드 차트 표현* : 사용자 설정 threshold 값을 참고용 수평 점선(threshold line)으로만 표시한다.
- *부재 설치 갯수 합계* = 현재까지 누적된 step 에서 설치된 `COUNT(기둥) + COUNT(거더)`
- *모델 전체 부재 갯수* = 모델의 모든 기둥과 거더가 설치 완료된 상태의 부재 설치 갯수 합계

### 시뮬레이션 출력 차트

- 선택된(1개 또는 여러개) 시나리오의 Metric XY Plot 을 출력한다.
- Metric XY Plot 의 x축은 step 이다.
- Metric XY Plot 의 y축은 각 층별 부재수 합계 또는 진행율, 혹은 누적 설치량이다.
- Step당 평균 추가 부재 수 추세선을 별도 KPI 라인으로 추가한다.

### 시나리오 비교 출력

- 시나리오 비교 테이블에는 “평균 연결성 점수”, “평균 Step 부재 수” 컬럼을 포함한다.
- 실패 원인 메타데이터에는 “독립 5개 단위 과다 사용” 항목을 포함한다.
- 필요시 시나리오별 총 step 수, 총 sequence 수, 층별 진행율 곡선을 함께 비교할 수 있다.

## 이 프로젝트의 기본 가정

작업조 투입수는 workfront 당 1개조. 이 프로젝트는 조립 순서만 구성한다.

단위 작업 시간은 하층기둥내부 콘크리트타설, 보, 바닥판 설치를 포함하므로 주부재 조립 순서의 조합만 연구한다.

Time & Cost 산정은 단위기간 공사 물량 기준으로 별도 산정 하므로 이번 프로젝트의 범위가 아니다.

1 Step 의 물리적인 Duration 은 없다. 즉, 부재 설치에 소요되는 시간은 이번 연구범위가 아니며 공사에 필요한 공기의 산정은 총 부재 설치 갯수와 작업 생산성을 고려하여 별도 계산으로 산정된다.

기둥 4개로 둘러싸인 바닥 면적에는 슬래브와 바닥 부부재(secondary member)들이 활성화 되는 것으로 가정한다.

임의의 층(레벨) 은 항상 flat 하여 같은 층에 존재하는 절점의 z 값은 동일하다.
