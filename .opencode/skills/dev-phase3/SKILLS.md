# SKILLS.md - Development Phase 3 Implementation

This document describes the technology stack, architecture, and implementation patterns used in AssyPlan Phase 3 (Simulation Mode).

## Overview

Phase 3 adds automatic construction sequence generation via Monte-Carlo simulation:
- Grid-based structural element pool generation (`sim_grid.rs`)
- Monte-Carlo + Pruning + Weighted Sampling algorithm (`sim_engine.rs`)
- Workfront-based parallel construction simulation
- Scenario comparison UI with playback controls
- Metric XY Plot (members-per-step trend)
- Rayon-based parallel scenario generation (8-core, 100 scenarios default)

## Technology Stack

### Rust Layer (New Modules)
- **Parallelism**: `rayon = "1"` — `into_par_iter()` for parallel scenario generation
- **Graphics**: `egui::Painter` — all charts drawn directly (no external chart crate)
- **eframe**: 0.27 (egui + wgpu)

## Module Architecture

### New Rust Modules (src/rust/src/)

#### sim_grid.rs
Generates the structural element pool from a Grid configuration.

**Key Types**:
```rust
pub struct SimGrid {
    pub nx: usize,             // x grid lines (≥2)
    pub ny: usize,             // y grid lines (≥2)
    pub nz: usize,             // z levels including ground
    pub dx: f64,               // x spacing
    pub dy: f64,               // y spacing
    pub dz: f64,               // floor height
    pub nodes: Vec<SimNode>,   // all grid nodes (1-indexed IDs)
    pub elements: Vec<SimElement>, // all columns + girders (1-indexed IDs)
}

pub struct SimNode {
    pub id: i32,   // 1-indexed
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub xi: usize, // grid x index
    pub yi: usize, // grid y index
    pub zi: usize, // floor level (0=ground)
}

pub struct SimElement {
    pub id: i32,            // 1-indexed
    pub node_i: i32,        // lower node ID
    pub node_j: i32,        // upper node ID
    pub member_type: SimMemberType, // Column or Girder
    pub floor: i32,         // 1-indexed floor level (column: lower end, girder: same z)
    pub xi: usize,          // grid x (column: bay x; girder: min of endpoints)
    pub yi: usize,          // grid y (column: bay y; girder: min of endpoints)
}

pub enum SimMemberType { Column, Girder }
```

**Constructor**:
```rust
SimGrid::new(nx, ny, nz, dx, dy, dz) -> SimGrid
```
- Nodes: all `nx × ny × nz` intersections, ID assigned row-major (x-major → y-major → z-major), 1-indexed
- Columns: between vertically adjacent nodes at same (xi, yi), all floors
- Girders: between horizontally adjacent nodes at same z level (both X-direction and Y-direction)
- Element IDs: columns first (all floors), then X-girders, then Y-girders, 1-indexed throughout

**Tests**: 11 unit tests in `sim_grid.rs` — all passing

#### sim_engine.rs
Monte-Carlo simulation engine for automatic construction sequence generation.

**Algorithm**:
```
Score = w1 × (1/member_count) + w2 × connectivity + w3 × (1/distance) + 0.05 × is_lowest_floor
```

**Candidate Priority Order** (higher = preferred):
1. 1 column + 1 girder
2. 1 column + 2 girders
3. 2 columns + 1 girder
4. 2 columns + 2 girders
5. 3 columns + 2 girders (independent minimum assembly — only when no connected candidates)

**Key Public API**:
```rust
pub struct Candidate {
    pub element_ids: Vec<i32>,
    pub member_count: usize,
    pub connectivity: f64,
    pub frontier_dist: f64,
    pub is_lowest_floor: bool,
    pub is_independent: bool,
}

impl Candidate {
    pub fn score(&self, w1: f64, w2: f64, w3: f64) -> f64
}

pub fn generate_incremental_candidates(
    wf: &SimWorkfront,
    grid: &SimGrid,
    installed_ids: &HashSet<i32>,
    installed_nodes: &HashSet<i32>,
) -> Vec<Candidate>

pub fn weighted_random_choice(scores: &[f64], rng_state: &mut u64) -> usize

pub fn run_scenario(
    scenario_id: usize,
    grid: &SimGrid,
    workfronts: &[SimWorkfront],
    seed: u64,
    weights: (f64, f64, f64),
    threshold: f64,       // 0.0~1.0 (internally converted to 0~100 for stability check)
) -> SimScenario

pub fn run_all_scenarios(
    count: usize,
    grid: &SimGrid,
    workfronts: &[SimWorkfront],
    weights: (f64, f64, f64),
    threshold: f64,
) -> Vec<SimScenario>   // parallel via rayon, sorted by scenario_id
```

**Early Termination Conditions**:
1. `consecutive_upper_floor_violations >= 3`
2. Fewer than 3 members added in last 300 global steps
3. `consecutive_independent >= 5` (independent 5-unit chosen ≥5 times)
4. `consecutive_no_candidates >= 10` (no valid candidate found 10 times)

**RNG**: LCG (Linear Congruential Generator) — no external `rand` crate needed.
Seed formula: `scenario_id as u64 * 2654435761` (Fibonacci hashing)

**Upper-floor check**: calls `stability::check_floor_installation_constraint()` with `threshold * 100.0` (function expects percentage, 0~100).

**Tests**: 8 unit tests in `sim_engine.rs`

#### graphics/sim_ui.rs
Simulation-specific UI panels (Settings, View, Result).

**Public API**:
```rust
pub fn render_sim_settings(ui: &mut Ui, state: &mut UiState) -> bool
pub fn render_sim_view(ui: &mut Ui, state: &mut UiState) -> bool
pub fn render_sim_result(ui: &mut Ui, state: &mut UiState)
```

**`render_sim_settings`**:
- Grid config sliders: nx(2~20), ny(2~20), nz(2~15), dx/dy(1000~20000mm), dz(1000~10000mm)
- Pool estimate label (computed inline from config)
- Upper-floor threshold slider (0.0~1.0)
- Algorithm weights (w1, w2, w3)
- Scenario count slider (1~200)
- Workfront list with remove buttons
- If grid config changes → clears workfronts + scenarios

**`render_sim_view`**:
- Draws x-y grid plan with `egui::Painter`
- Click grid intersections to add/remove workfronts (toggle)
- Shows workfront markers (colored circles with WF# label)
- Info overlay: grid size, floor count, workfront count
- Returns true if workfront list changed

**`render_sim_result`**:
- Scrollable scenario table (ID, Steps, Members, Avg/Step, Termination)
- Click row to select scenario
- Playback controls: ◀ ▶ ▶Play ⏸Pause + speed selector (1×/2×/4×)
- Step seek slider
- Step info (WF, Floor, element IDs)
- Metrics summary grid
- XY Plot: members-per-step (internal `render_members_per_step_plot`)

**`render_members_per_step_plot` (private)**:
- X = step index, Y = members installed at that step
- Target band 1.8~2.4 (semi-transparent green)
- Mean line (red dashed) with "avg X.X" label
- Dashed grid lines for Y axis
- Legend (top-right)

#### graphics/ui.rs (Extended for Phase 3)

**New Types**:
```rust
pub struct GridConfig {
    pub nx: usize, pub ny: usize, pub nz: usize,
    pub dx: f64, pub dy: f64, pub dz: f64,
}
impl Default for GridConfig { /* nx=4,ny=4,nz=3, dx=dy=6000,dz=4000 */ }

pub struct SimWorkfront {
    pub id: i32,         // 1-indexed
    pub grid_x: usize,   // 0-indexed
    pub grid_y: usize,   // 0-indexed
}

pub enum TerminationReason {
    Completed, UpperFloorViolation, NoProgress, IndependentOveruse, NoCandidates, MaxIterations,
}

pub struct ScenarioMetrics {
    pub avg_members_per_step: f64,
    pub avg_connectivity: f64,
    pub total_steps: usize,
    pub total_members_installed: usize,
    pub termination_reason: TerminationReason,
}

// SimSequence — individual element within a step (1-indexed globally)
pub struct SimSequence {
    pub element_id: i32,
    pub sequence_number: usize,  // 1-indexed, global across all steps
}

// SimStep — one pattern-based installation unit
pub struct SimStep {
    pub workfront_id: i32,
    pub element_ids: Vec<i32>,       // backward compat (= sequences.iter().map(|s| s.element_id))
    pub sequences: Vec<SimSequence>, // individual sequence entries within this step
    pub floor: i32,
    pub pattern: String,             // e.g. "ColCol", "ColGirder", "Bootstrap", etc.
}

impl SimStep {
    /// Helper: build from element_ids + pattern, auto-generating sequences
    pub fn from_elements(workfront_id, element_ids, floor, pattern, start_seq) -> SimStep
}

pub struct SimScenario {
    pub id: usize,    // 1-indexed
    pub seed: u64,
    pub steps: Vec<SimStep>,
    pub metrics: ScenarioMetrics,
}
```

**New UiState Fields**:
```rust
pub grid_config: GridConfig,
pub sim_workfronts: Vec<SimWorkfront>,
pub sim_scenarios: Vec<SimScenario>,
pub sim_selected_scenario: Option<usize>,  // 0-based index
pub sim_playing: bool,
pub sim_speed: u32,          // 1, 2, or 4
pub sim_current_step: usize, // 1-indexed
pub sim_play_timer: f64,
pub sim_weights: (f64, f64, f64),  // default (0.5, 0.3, 0.2) — sum = 1.0
pub sim_running: bool,
pub sim_scenario_count: usize,     // default 100
```

#### lib.rs (Extended for Phase 3)

**New mod declarations (top)**:
```rust
pub mod sim_engine;
pub mod sim_grid;
```

**Simulation branch in `recalculate()`**:
```rust
fn recalculate(&mut self) {
    if !self.ui_state.has_data && self.ui_state.display_mode != graphics::DisplayMode::Simulation {
        // no data error...
        return;
    }

    if self.ui_state.display_mode == graphics::DisplayMode::Simulation {
        self.run_simulation();
        return;
    }
    // ... existing Development mode logic
}
```

**`run_simulation()` (new)**:
```rust
fn run_simulation(&mut self) {
    // Build grid from grid_config
    let grid = sim_grid::SimGrid::new(cfg.nx, cfg.ny, cfg.nz, cfg.dx, cfg.dy, cfg.dz);
    // Default workfront if none set: (0,0)
    // Call sim_engine::run_all_scenarios() (rayon parallel)
    // Select best scenario (most members installed)
    // Update ui_state.sim_scenarios, sim_selected_scenario, sim_running, status_message
}
```

**Result tab** (`"Result"` branch):
```rust
"Result" => {
    if self.ui_state.display_mode == graphics::DisplayMode::Simulation {
        graphics::sim_ui::render_sim_result(ui, &mut self.ui_state);
    } else {
        graphics::render_result_tab(ui, &self.ui_state);
    }
}
```

## Data Flow Architecture

```
Settings tab (GridConfig, weights, scenario count, upper_floor_threshold)
    |
    v
View tab (click grid intersections → SimWorkfront list)
    |
    v
[Recalc button] → recalculate() → run_simulation()
    |
    v
sim_grid::SimGrid::new(nx, ny, nz, dx, dy, dz) → grid + element pool
    |
    v
sim_engine::run_all_scenarios(count, &grid, &workfronts, weights, threshold)
    |  ← rayon par_iter (parallel, 8 cores default)
    v
Vec<SimScenario> (sorted by ID)
    |
    v
UiState.sim_scenarios + sim_selected_scenario (best scenario auto-selected)
    |
    v
Result tab → render_sim_result(ui, state)
    |
    ├── Scenario table (click to select)
    ├── Playback controls
    ├── Metrics summary
    └── XY Plot (members/step, target band 1.8~2.4, mean line)
```

## Known Gotchas and Constraints

### threshold conversion
`check_floor_installation_constraint()` in `stability.rs` expects `threshold_percentage` in **0~100 range**.
`upper_floor_threshold` in `UiState` is stored as **0.0~1.0**.
Conversion in `sim_engine.rs`: `threshold * 100.0`.

### run_simulation() requires no CSV data
Simulation Mode generates its own element pool from `GridConfig`. `has_data` is NOT required.
The guard in `recalculate()` skips the `!has_data` check when `display_mode == Simulation`.

### Default workfront
If `sim_workfronts` is empty when `run_simulation()` is called, a default workfront at `(grid_x=0, grid_y=0)` is created automatically.

### Scenario ID is 1-indexed
`SimScenario.id` starts at 1. `sim_selected_scenario` stores the **0-based index** into `sim_scenarios`.
The table row uses `scenario.id - 1` to set `sim_selected_scenario`.

### rayon + eframe thread safety
`run_all_scenarios()` uses `rayon::par_iter()`. `SimGrid` and `SimWorkfront` are `Send + Sync` (no interior mutability). Called synchronously from `update()` — blocks the UI thread during computation. For large grids (nx/ny/nz > 10), the frame freezes briefly. Future: move to background thread.

### GridConfig change clears workfronts
When `render_sim_settings` detects config change, it clears `sim_workfronts` and `sim_scenarios`. This prevents out-of-range workfront indices.

### Unused `GridConfig` import warning
`sim_ui.rs` imports `GridConfig` but doesn't use it directly (accessed via `state.grid_config`). This is a pre-existing warning — do not suppress with `#[allow(unused_imports)]`, fix by removing the import if refactoring.

### console build cfg_attr (`main.rs`)
```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "console")]
```
This allows users to see `println!` debug output even in release builds. Do NOT change to `"windows"` — that suppresses the console window entirely.

### SimStep pattern field
`SimStep.pattern` is a human-readable string: `"Bootstrap"`, `"Col"`, `"Girder"`, `"ColCol"`, `"ColGirder"`, `"GirderGirder"`, `"ColColGirder"`, `"ColGirderCol"`, `"ColGirderGirder"`, `"ColColGirderGirder"`, `"ColColGirderColGirder"`. Used for display in Result tab step info.

### Forbidden patterns (sim_engine.rs)
`try_build_pattern()` must NEVER produce:
- 3 consecutive Columns without any Girder between them: `Col→Col→Col`
- 4 consecutive elements with 3 Columns: `Col→Col→Col→Girder`
These are blocked in the pattern builder by checking `consecutive_col_count`.

### Inactive element ghost color
Construction mode sim 3D view inactive elements use `Color32::from_gray(90)` (NOT 38 — too dark to see on monitor).

### sim 3D view orbit — single `allocate_rect`
In the sim 3D view (inlined in `lib.rs` View tab), use a SINGLE `allocate_rect` call. A second `allocate_rect` for the same rect causes double orbit processing and erratic camera behavior. The hover check must use `ui.input(|i| i.pointer.hover_pos()).map(|p| rect_3d.contains(p))` directly.

### Construction mode element ID display filtering
In Construction Sequence/Step modes, node/element IDs must only be shown for **installed** elements (not ghost/inactive). Loop over installed element IDs only, not all elements. Otherwise IDs appear for elements not yet visible on screen.

## File Structure Changes (Phase 3 Additions)

```
src/rust/
├── Cargo.toml              MODIFIED: added rayon = "1"
└── src/
    ├── lib.rs              MODIFIED: pub mod sim_engine, sim_grid; run_simulation(); Result tab branch
    ├── sim_grid.rs         NEW: grid node/element pool generation
    ├── sim_engine.rs       NEW: Monte-Carlo algorithm + rayon parallel execution
    └── graphics/
        ├── mod.rs          MODIFIED: pub mod sim_ui
        ├── sim_ui.rs       NEW: Settings/View/Result tabs for Simulation Mode
        └── ui.rs           MODIFIED: DisplayMode::Simulation; GridConfig/SimWorkfront/SimScenario types; UiState fields
```

## Phase 3 Completion Status

| Feature | Status |
|---------|--------|
| DisplayMode::Simulation added | ✅ |
| Simulation button activated | ✅ |
| GridConfig + UiState Sim fields | ✅ |
| sim_grid.rs (pool generation) | ✅ |
| sim_ui.rs (Settings/View/Result) | ✅ |
| sim_engine.rs (Monte-Carlo, rayon) | ✅ |
| recalculate() Simulation branch | ✅ |
| rayon parallel scenarios | ✅ |
| Result tab render_sim_result | ✅ |
| XY Plot (egui::Painter) | ✅ |
| Release build passing | ✅ |
| Sim View 3D render (lib.rs View tab, inlined) | ✅ |
| Scenario comparison chart (render_scenario_comparison_chart) | ✅ |
| Debug file export (CSV per scenario + summary.txt) | ✅ |
| SimSequence/SimStep separation (pattern-based steps) | ✅ |
| Forbidden pattern enforcement (Col→Col→Col blocked) | ✅ |
| Scenario ComboBox dropdown in sim 3D view nav bar | ✅ |
| Inactive element color fix (gray 38→90) | ✅ |
| Sim 3D view orbit unification (single allocate_rect) | ✅ |
| Construction mode ID display filtering (installed only) | ✅ |
| Console build (cfg_attr windows_subsystem = "console") | ✅ |

### Sim View 3D Render — Implementation Notes
- Inlined directly in `lib.rs` View tab (NOT in sim_ui.rs — dead code `render_sim_3d` was removed)
- When `DisplayMode::Simulation` and `sim_grid.is_some()` and a scenario is selected:
  - Top panel: 3D render of installed elements at `sim_current_step` (orbit/zoom/pan via existing `ViewState`)
  - Bottom panel: 2D grid plan with clickable workfront intersections
- Element rendering uses `e.member_type == "Column"` string comparison (NOT enum)
- Node access: `e.node_i_id` / `e.node_j_id` (SimElement fields, NOT `node_i`/`node_j`)
- Grid stored as `self.sim_grid: Option<SimGrid>` on `AssyPlanApp` (NOT on `UiState`)
- When storing: `self.sim_grid = Some(grid.clone())` to avoid borrow-after-move

### Scenario Comparison Chart — Implementation Notes
- Defined as `render_scenario_comparison_chart(ui, state)` in `sim_ui.rs`
- Called at the bottom of `render_sim_result()` when `sim_scenarios.len() > 1`
- Multi-line XY plot: X = step, Y = cumulative members installed
- Shows top-10 scenarios (sorted by total_members_installed descending)
- Selected scenario highlighted with thicker line + label
- Drawn with `egui::Painter` directly (no external chart crate)

## Next Steps (Phase 3+)

- **Playback animation**: auto-advance `sim_current_step` using `sim_play_timer` + `ctx.request_repaint()` — explicitly deferred (not this session)
- **get_floor_level() z-map caching**: see AGENTS.md §9.1 for optimization plan
- **Background thread simulation**: prevent UI freeze during `run_all_scenarios()` on large grids
