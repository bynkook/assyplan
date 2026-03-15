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
- **[Added]** Upper-Floor Column Installation Rate metric (Chart 3)
- **[Added]** Construction constraint threshold slider in Settings tab

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

#### stability.rs
Core stability validation, table generation, and floor-level analysis.

**Key Public Functions**:
- `get_floor_level(node_id, nodes)`: Returns 1-indexed floor number for a node by sorting unique z values
- `get_column_floor(element, nodes)`: Returns floor of a column using its i-node (lower node) z coordinate
- `get_floor_column_counts(elements, nodes)`: Returns `HashMap<i32, usize>` mapping floor → total column count
- `check_floor_installation_constraint(target_floor, installed_ids, all_elements, nodes, threshold_percentage)`: Returns `(allowed: bool, lower_floor_pct: f64)`
- `build_step_elements_map(sequence_table, step_table, elements, nodes)`: Returns `Vec<Vec<(i32, String, i32)>>` indexed by step — each entry = `(element_id, member_type, floor_level)`
- `generate_all_tables(nodes, elements)`: Main entry point for table generation
- `get_floor_column_data(elements, nodes, max_step, step_elements)`: Returns `Vec<(i32, usize, usize)>` = (floor, total_columns, installed_at_max_step)

**Floor Level Logic** (important):
```rust
pub fn get_floor_level(node_id: i32, nodes: &[StabilityNode]) -> i32 {
    // Collect all unique z values across ALL nodes
    let mut unique_z: Vec<i64> = nodes.iter()
        .map(|n| (n.z * 1000.0) as i64)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    unique_z.sort();
    
    // Find this node's z and return 1-indexed position
    let node_z = nodes.iter().find(|n| n.id == node_id).map(|n| (n.z * 1000.0) as i64).unwrap_or(0);
    unique_z.iter().position(|&z| z == node_z).map(|p| p as i32 + 1).unwrap_or(1)
}
```
- **Performance note**: Called O(N×M) times during table generation. Each call scans all nodes and sorts. For large structures (10k+ elements), cache the z-value map as `HashMap<i64, i32>` and pass it as a parameter instead.

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
Step navigation UI, metrics display, and 3 charts.

**UiState Extensions** (Phase 2 additions):
```rust
pub struct UiState {
    // ... existing Phase 1 fields ...
    pub current_step: usize,          // Current step (1-indexed)
    pub max_step: usize,              // Maximum step
    pub step_input: String,           // Direct input field value
    pub has_step_data: bool,          // Whether step data calculated
    pub construction_view_mode: ConstructionViewMode,  // Sequence or Step
    pub current_sequence: usize,      // 1-indexed sequence position
    pub max_sequence: usize,          // Total elements
    pub total_elements: usize,        // Metric: total
    pub total_columns: usize,         // Metric: columns
    pub total_girders: usize,         // Metric: girders
    pub workfront_count: usize,       // Metric: workfronts
    pub floor_column_data: Vec<(i32, usize, usize)>,  // (floor, total, installed)
    pub needs_recalc: bool,           // Recalc flag
    pub step_elements: Vec<Vec<(i32, String, i32)>>,  // Per-step elements
    pub upper_floor_threshold: f64,   // 0.0~1.0, default 0.3 (30%)
}
```

**Charts in Result Tab** (`render_result_tab_inner`):

| Chart | Title | X-axis | Y-axis | Description |
|-------|-------|--------|--------|-------------|
| Chart 1 | Element Type Distribution | Step | Count | Cumulative columns vs girders per step |
| Chart 2 | Floor-by-Floor Installation Progress | Step | Count | Per-floor column count per step |
| Chart 3 | Upper-Floor Column Installation Rate | Step | 0~100% | For floor N: (N+1 cumulative installs) / (N cumulative installs); threshold line |

**Chart 3 Details** (Upper-Floor Column Installation Rate):
- Formula: **(cumulative columns installed at floor N+1) / (cumulative columns installed at floor N)**
- When denominator (floor N columns) = 0: rate is **0.0** (not 1.0)
- Legend labels: `F{N+1}/F{N}` (e.g., "F2/F1" means floor 2 installs ÷ floor 1 installs)
- When a line crosses the threshold, floor N+1 columns are "getting ahead" of floor N
- Threshold: configurable in Settings tab (default **30%**)
- Threshold shown as red dashed line (manually drawn, 6px dash / 4px gap)
- `floor_colors` array defined at `render_result_tab_inner` function scope — shared by Chart 2 and Chart 3

**floor_colors array** (defined at function scope, not inside chart closure):
```rust
let floor_colors = [
    Color32::from_rgb(100, 200, 255),  // light blue
    Color32::from_rgb(100, 255, 150),  // light green
    Color32::from_rgb(255, 200, 100),  // amber
    Color32::from_rgb(255, 120, 200),  // pink
    Color32::from_rgb(180, 130, 255),  // lavender
    Color32::from_rgb(255, 255, 100),  // yellow
];
```
⚠️ Defining `floor_colors` inside a chart's closure makes it inaccessible to other charts. Always define it at function scope.

**Settings Tab — Construction Constraints**:
```rust
ui.heading("Construction Constraints");
ui.add(
    egui::Slider::new(&mut state.upper_floor_threshold, 0.0..=1.0)
        .text("")
        .fixed_decimals(2)
        .clamp_to_range(true),
);
ui.label(format!("{:.0}%", state.upper_floor_threshold * 100.0));
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
[PyO3 bindings / stability.rs] --> Tables + step_elements + floor_column_data
    |
    v
[ui.rs UiState] --> upper_floor_threshold, step_elements, floor_column_data
    |
    v
[render_result_tab_inner] --> Chart 1, Chart 2, Chart 3
    |
    v
[egui::Painter] --> Line charts drawn via painter.add(Shape::line(...))
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

### get_floor_level() Performance Warning
`get_floor_level()` in `stability.rs` scans all nodes and sorts unique z values on **every call**.
- Current scale (hundreds of elements): acceptable
- Large scale (10k+ elements): O(N×M) becomes a bottleneck

**Future optimization**:
```rust
// Build z-map once before any floor_level calls
let z_map: HashMap<i64, i32> = build_z_level_map(nodes);
// Then pass z_map to all floor-level functions instead of recomputing
```

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

### Rust Tests (src/rust/src/stability.rs and step_renderer.rs)
- `test_validate_minimum_assembly_valid`: Minimum assembly unit
- `test_validate_column_support_ground_level`: Column ground support
- `test_validate_girder_support`: Girder support
- `test_floor_level_calculation`: Floor numbering correctness
- `test_floor_installation_constraint`: check_floor_installation_constraint()
- `test_generate_all_tables`: Full table generation
- `test_step_table_for_rendering`: Rendering-ready data format
- `test_step_render_data_new`: StepRenderData constructor
- `test_cumulative_elements`: Cumulative calculation
- `test_cumulative_cache`: Cache behavior
- `test_cache_invalidation`: Cache clearing

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

### floor_colors Scope (IMPORTANT)
- `floor_colors` must be defined **outside** all chart closures/frames
- Defined at `render_result_tab_inner` function scope
- If placed inside a closure (e.g., Chart 2's `egui::Frame::none().show(...)`), it will be inaccessible to Chart 3
- This was a bug found during Chart 3 implementation — fixed by moving the definition up

### Chart Dashed Lines (no external crate)
egui has no built-in dashed line primitive. Draw manually:
```rust
let dash_len = 6.0f32;
let gap_len = 4.0f32;
let mut x = plot_rect.left();
while x < plot_rect.right() {
    let x_end = (x + dash_len).min(plot_rect.right());
    painter.line_segment([pos2(x, y), pos2(x_end, y)], stroke);
    x += dash_len + gap_len;
}
```

### upper_floor_threshold Metric vs Constraint
- `upper_floor_threshold` controls Chart 3 threshold line visualization ONLY
- Formula: `(cumulative installs at floor N+1) / (cumulative installs at floor N)`, denominator=0 → 0.0
- Default value: **0.3 (30%)**
- Actual construction sequence is NOT re-generated when threshold changes (Phase 2 limitation)
- Full constraint enforcement with sequence re-generation is planned for Phase 3 (Simulation Mode)

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
│           ├── stability.rs       # NEW: Full stability + table generation
│           └── graphics/
│               ├── step_renderer.rs # NEW: Step rendering + cache
│               ├── view_state.rs    # NEW: 3D viewport state
│               ├── axis_cube.rs     # NEW: Orientation cube
│               └── ui.rs          # EXTENDED: Step nav, Charts 1-3, Settings
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
- Phase 3: Automatic step generation algorithm with upper-floor threshold constraint enforcement
- Phase 3: Workfront expansion direction selection
- Phase 4: Interactive editing and member modification
- Phase 4: get_floor_level() → z-map caching optimization for large structures
