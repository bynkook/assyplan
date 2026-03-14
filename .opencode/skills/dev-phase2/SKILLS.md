# SKILLS.md - Development Phase 2 Implementation

This document describes the technology stack, architecture, and implementation patterns used in AssyPlan Development Phase 2.

## Overview

Phase 2 adds Step-based construction visualization to the Development Mode:
- Precedent member DAG (Directed Acyclic Graph) parsing
- Workfront identification
- Stability condition validation
- Construction Sequence Table and Workfront Step Table generation
- Step-by-step cumulative rendering with caching (60fps target)
- Step navigation UI (prev/next/slider/direct input)

## Technology Stack

### Python Layer (New Modules)
- **Version**: Python 3.12+
- **Data Processing**: pandas 2.0+, numpy 1.26+
- **Graph Operations**: Custom DAG implementation (no external dependency)
- **Testing**: pytest 8.0+

### Rust Layer (Extended)
- **Graphics Framework**: eframe 0.27 (egui + wgpu)
- **Caching**: `std::collections::HashMap` for cumulative element cache
- **Python Bindings**: PyO3 0.21

## Module Architecture

### Python Modules (src/python/)

#### precedent_graph.py
Builds a Directed Acyclic Graph from precedent member relationships.

**Key Functions**:
- `build_dag(df: pd.DataFrame) -> DAG`: Constructs graph from `선행부재ID` column
- `detect_cycles(dag: DAG) -> bool`: Detects circular references
- `get_predecessors(dag: DAG, member_id: str) -> List[str]`: Returns predecessor members
- `get_successors(dag: DAG, member_id: str) -> List[str]`: Returns successor members

**DAG Structure**:
```python
class DAG:
    nodes: Set[str]           # All member IDs
    edges: Dict[str, Set[str]]  # member_id -> set of successor member_ids
    reverse_edges: Dict[str, Set[str]]  # member_id -> set of predecessor member_ids
```

**Cycle Detection**:
- Uses Kahn's algorithm (topological sort) to detect cycles
- If topological sort doesn't include all nodes, cycle exists
- Raises `ValueError("Cycle detected in precedent graph")` on cycle

#### workfront.py
Identifies workfront starting points from precedent member data.

**Key Functions**:
- `identify_workfronts(df: pd.DataFrame) -> List[int]`: Returns workfront IDs (1-indexed)
- `get_workfront_members(df: pd.DataFrame, dag: DAG) -> Dict[int, List[str]]`: Maps workfront_id to member list

**Workfront Identification Rules**:
- Members with empty `선행부재ID` = new workfront starting points
- Workfront ID assigned sequentially starting from 1
- Each workfront includes all reachable successors in the DAG

#### stability_validators.py
Validates structural stability conditions for step assignment.

**Validation Functions**:
- `validate_minimum_assembly(nodes, elements)`: Checks for minimum assembly unit (3 columns + 2 girders at 90 degrees)
- `validate_column_support(element, nodes, assembled)`: Checks column i-node is ground or connected to assembled column j-node
- `validate_girder_support(element, nodes, assembled)`: Checks girder both ends connected to stable columns/girders
- `validate_no_ground_girder(elements, nodes)`: No girders at z=0 level

**Minimum Assembly Unit**:
```
        Girder (G2)
    Col2 -------- Col3
      |            |
      |  Girder    |
      |   (G1)     |
    Col1 -------- (implicit)
```
- Requires 3 columns arranged in L-shape or line
- Requires 2 girders connecting the columns at 90 degrees
- This forms the minimum stable structure

**Column Support Rule** (devplandoc.md:228-232):
- Column i-node must be at ground level (z=0), OR
- Column i-node must connect to an already-assembled column's j-node

**Girder Support Rule** (devplandoc.md:233-236):
- Both girder end nodes must connect to stable (already-assembled) columns or girders

#### sequence_table.py
Generates Construction Sequence Table using topological sort.

**Key Functions**:
- `create_sequence_table(dag: DAG, workfronts: Dict) -> List[Tuple[int, str]]`: Returns (workfront_id, member_id) pairs
- `topological_sort(dag: DAG) -> List[str]`: Kahn's algorithm for topological ordering

**Output Format**:
```python
[
    (1, "1CF001"),  # workfront 1, first member
    (1, "2GF001"),  # workfront 1, second member
    ...
]
```

#### step_table.py
Assigns step numbers based on stability conditions.

**Key Functions**:
- `create_step_table(sequence, nodes, elements, dag) -> List[Tuple[int, int, str]]`: Returns (workfront_id, step, member_id)
- `assign_steps(sequence, nodes, elements) -> List[Tuple[int, int, str]]`: Core step assignment algorithm

**Step Assignment Algorithm**:
```python
def assign_steps(sequence, nodes, elements):
    assembled = set()  # Already assembled member IDs
    current_step = 1
    result = []
    
    for workfront_id, member_id in sequence:
        element = get_element(member_id)
        
        # Check if member can be assembled at current step
        if is_stable(element, assembled):
            # Can assemble now
            result.append((workfront_id, current_step, member_id))
            assembled.add(member_id)
        else:
            # Need to wait - increment step and try again
            current_step += 1
            result.append((workfront_id, current_step, member_id))
            assembled.add(member_id)
    
    return result
```

**Step Boundary Rules**:
- Step increments when the next member cannot be assembled with current assembled set
- Multiple members can share the same step if all can be assembled simultaneously
- Steps start from 1 (never 0)

### Rust Modules (src/rust/src/)

#### graphics/step_renderer.rs
Step-based cumulative rendering with caching.

**Core Types**:
```rust
pub struct StepRenderData {
    pub base: RenderData,                    // Nodes and elements
    pub step_elements: Vec<Vec<usize>>,      // step_elements[step-1] = element indices
    pub current_step: usize,                 // Currently viewing (1-indexed)
    pub max_step: usize,                     // Maximum step number
    cumulative_cache: HashMap<usize, Vec<usize>>, // Cache for cumulative elements
}
```

**Key Methods**:
- `set_step_data(step_table)`: Initializes step data from Python
- `get_elements_for_step(step)`: Non-cumulative elements for specific step
- `get_cumulative_elements(step)`: Cumulative elements from step 1 to step (cached)
- `set_current_step(step)`: Direct navigation
- `next_step()`, `prev_step()`: Sequential navigation
- `render_step(painter, rect)`: Renders current step view

**Cumulative Rendering**:
- Step N shows all elements from Step 1 to Step N
- Previous step elements: grey color (Color32::from_gray(150))
- Current step elements: original colors (Column: red, Girder: green)
- Grid and nodes: always fully visible

**Caching Strategy**:
```rust
pub fn get_cumulative_elements(&mut self, step: usize) -> Vec<usize> {
    // Check cache first - O(1)
    if let Some(cached) = self.cumulative_cache.get(&step) {
        return cached.clone();
    }
    
    // Calculate cumulative elements - O(n)
    let mut cumulative = Vec::new();
    for s in 1..=step {
        cumulative.extend_from_slice(&self.step_elements[s-1]);
    }
    
    // Cache for next time
    self.cumulative_cache.insert(step, cumulative.clone());
    cumulative
}
```

**Cache Invalidation**:
- `invalidate_cache()`: Called when step_elements changes
- `set_step_data()`: Automatically invalidates cache
- Slider/button navigation reuses cache (no invalidation needed)

#### graphics/ui.rs (Extended)
Step navigation UI components.

**UiState Extensions**:
```rust
pub struct UiState {
    // ... existing fields ...
    pub current_step: usize,    // Current step (1-indexed)
    pub max_step: usize,        // Maximum step
    pub step_input: String,     // Direct input field value
}
```

**Step Navigation UI**:
- Previous button (◀): `state.current_step - 1`, disabled at step 1
- Next button (▶): `state.current_step + 1`, disabled at max_step
- Slider: `egui::Slider::new(&mut step, 1..=max_step)`
- Direct input: `egui::TextEdit::singleline(&mut step_input)`

**Input Validation**:
```rust
if let Ok(step) = state.step_input.parse::<usize>() {
    let clamped = step.max(1).min(state.max_step);
    state.current_step = clamped;
    state.step_input = clamped.to_string();
}
```

#### lib.rs (Extended)
PyO3 bindings for step data.

**New PyClasses**:
```rust
#[pyclass]
pub struct PyStepTable {
    pub entries: Vec<(i32, usize, String)>, // (workfront_id, step, member_id)
    pub max_step: usize,
}
```

**New PyFunctions**:
- `load_step_data(step_table: &PyStepTable) -> PyResult<usize>`: Load step data
- `get_step_table_info(step_table: &PyStepTable) -> PyResult<String>`: Get info
- `create_step_table(entries: Vec<(i32, usize, String)>) -> PyResult<PyStepTable>`: Create from Python list

## Data Flow Architecture

```
CSV File (with 선행부재ID)
    |
    v
[data_loader.py] --> DataFrame
    |
    v
[precedent_graph.py] --> DAG (nodes + edges)
    |
    v
[workfront.py] --> Workfront identification
    |
    v
[sequence_table.py] --> Construction Sequence (topological order)
    |
    v
[step_table.py] --> Workfront Step Table (with stability validation)
    |
    v
[PyO3 bindings] --> PyStepTable
    |
    v
[step_renderer.rs] --> StepRenderData
    |
    v
[ui.rs] --> Step navigation controls
    |
    v
[egui::Painter] --> Cumulative render
```

## Step Assignment Algorithm

### Topological Sort (Kahn's Algorithm)
```python
def topological_sort(dag):
    in_degree = {node: 0 for node in dag.nodes}
    for successors in dag.edges.values():
        for succ in successors:
            in_degree[succ] += 1
    
    queue = [n for n in dag.nodes if in_degree[n] == 0]
    result = []
    
    while queue:
        node = queue.pop(0)
        result.append(node)
        for succ in dag.edges.get(node, []):
            in_degree[succ] -= 1
            if in_degree[succ] == 0:
                queue.append(succ)
    
    if len(result) != len(dag.nodes):
        raise ValueError("Cycle detected")
    
    return result
```

### Stability-Based Step Assignment
```
For each member in topological order:
    1. Check if member can be assembled with current assembled set
    2. If stable: add to current step
    3. If not stable: increment step, then add
    4. Mark member as assembled
```

## Performance Considerations

### 60fps Target for 10,000 Elements
Key optimizations:
1. **Cumulative cache**: O(1) lookup for repeated step views
2. **Incremental invalidation**: Only invalidate cache when step data changes
3. **Lazy evaluation**: Cumulative elements computed only when needed
4. **Clipping**: Only render elements within viewport rect

### Memory Management
- Cache stores Vec<usize> (element indices), not full element data
- Cache entries: O(max_step) in worst case
- Each entry: O(n) where n = cumulative element count

## Testing Strategy

### Python Tests (tests/python/)
- `test_precedent_graph.py`: DAG construction, cycle detection
- `test_workfront.py`: Workfront identification
- `test_stability_validators.py`: All 4 validation functions
- `test_sequence_table.py`: Topological sort, sequence generation
- `test_step_table.py`: Step assignment, edge cases
- `test_phase2.py`: E2E integration tests

### Rust Tests (src/rust/src/graphics/step_renderer.rs)
- `test_step_render_data_new`: Constructor
- `test_set_step_data`: Data loading
- `test_cumulative_elements`: Cumulative calculation
- `test_cumulative_cache`: Cache behavior
- `test_cache_invalidation`: Cache clearing
- `test_set_current_step`: Direct navigation
- `test_next_step`, `test_prev_step`: Sequential navigation
- `test_boundary_conditions`: Edge cases
- `test_step_info`: Info formatting
- `test_has_steps`: State checking
- `test_element_counts`: Count calculations
- `test_single_step`: Single step model
- Integration tests: UI state sync, cache with navigation, etc.

## Integration Patterns

### Python → Rust Data Transfer
```python
# Python side
from step_table import create_step_table
step_table = create_step_table(sequence, nodes, elements, dag)

# Convert to Rust format
import assyplan
py_step_table = assyplan.create_step_table([
    (wf_id, step, member_id) for wf_id, step, member_id in step_table
])
assyplan.load_step_data(py_step_table)
```

### UI State Synchronization
```rust
// When Python sends new step data
fn on_step_data_loaded(&mut self, step_table: &PyStepTable) {
    self.step_render_data.set_step_data(step_table.entries.clone());
    self.ui_state.set_step_data(step_table.max_step);
}

// When user navigates
fn on_step_change(&mut self, new_step: usize) {
    if self.step_render_data.set_current_step(new_step) {
        self.ui_state.current_step = new_step;
        self.ui_state.step_input = new_step.to_string();
    }
}
```

## Known Gotchas and Workarounds

### Cycle Detection in Precedent Graph
- User data may contain invalid circular references
- Always run cycle detection before step assignment
- Clear error message: "Cycle detected involving members: [list]"

### Step 0 Prevention
- All step numbers are 1-indexed (never 0)
- UI validation clamps input to `max(1, min(input, max_step))`
- Rust `saturating_sub(1)` for safe index conversion

### Empty Precedent ID Handling
- Empty string, None, NaN all treated as "no predecessor"
- These members become workfront starting points
- Multiple workfronts can exist in a single model

### Cache Consistency
- Cache invalidation on ANY step_elements modification
- Navigation (next/prev/slider) does NOT invalidate cache
- Only `set_step_data()` triggers invalidation

## Project Structure (Phase 2 Additions)

```
assyplan/
├── src/
│   ├── python/
│   │   ├── precedent_graph.py     # NEW: DAG construction
│   │   ├── workfront.py           # NEW: Workfront identification
│   │   ├── stability_validators.py # NEW: Stability validation
│   │   ├── sequence_table.py      # NEW: Construction sequence
│   │   └── step_table.py          # NEW: Step assignment
│   └── rust/
│       └── src/
│           ├── lib.rs             # EXTENDED: PyStepTable, new bindings
│           └── graphics/
│               ├── step_renderer.rs # NEW: Step rendering + cache
│               └── ui.rs          # EXTENDED: Step navigation UI
├── tests/
│   └── python/
│       ├── test_precedent_graph.py     # NEW
│       ├── test_workfront.py           # NEW
│       ├── test_stability_validators.py # NEW
│       ├── test_sequence_table.py      # NEW
│       ├── test_step_table.py          # NEW
│       └── test_phase2.py              # NEW: E2E integration
└── .opencode/skills/
    └── dev-phase2/
        └── SKILLS.md              # This file
```

## Next Steps / Future Phases

- Phase 3: Simulation Mode activation
- Phase 3: Automatic step generation algorithm
- Phase 3: Workfront expansion direction selection
- Phase 4: Interactive editing and member modification
