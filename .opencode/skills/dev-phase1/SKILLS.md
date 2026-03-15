# SKILLS.md - Development Phase 1 Implementation

This document describes the technology stack, architecture, and implementation patterns used in AssyPlan Development Phase 1.

## Technology Stack

### Python Layer
- **Version**: Python 3.12+
- **Data Processing**: pandas 2.0+, numpy 1.26+
- **Encoding Detection**: charset_normalizer 3.0+
- **Build Bridge**: maturin 1.4+ (PyO3 C extension builder)
- **Testing**: pytest 8.0+

### Rust Layer
- **Graphics Framework**: eframe 0.27 (egui + wgpu integration)
- **Python Bindings**: PyO3 0.21 (cdylib + rlib)
- **Edition**: 2021

### Build System
- Single-source, multi-platform build with maturin
- Python module: `assyplan._core` (Rust extension)
- Supports both direct Rust execution and Python integration

## Module Architecture

### Python Modules (src/python/)

#### data_loader.py
Handles CSV file import with automatic encoding detection.

**Key Functions**:
- `load_csv(filepath: str) -> pd.DataFrame`: Detects encoding using charset_normalizer, loads CSV with pandas, validates required columns

**Required Columns**:
- `부재ID`: Member ID (Korean)
- `node_i_x`, `node_i_y`, `node_i_z`: Start node coordinates
- `node_j_x`, `node_j_y`, `node_j_z`: End node coordinates
- `선행부재ID`: Predecessor member ID

**Gotcha**: charset_normalizer returns encoding names with underscores (e.g., "utf_8") not dashes (e.g., "utf-8").

#### node_table.py
Extracts unique nodes from element data and assigns sequential IDs.

**Key Functions**:
- `create_node_table(df: pd.DataFrame) -> List[Tuple[int, float, float, float]]`: Returns (node_id, x, y, z) tuples

**ID Assignment Strategy**:
- Collects all unique coordinates from node_i and node_j columns
- Sorts by (x, y, z) in ascending order
- Assigns IDs starting from 1 (not 0)
- This ensures consistent, reproducible node numbering

#### element_table.py
Maps structural members to elements with member type classification.

**Key Functions**:
- `create_element_table(df, nodes) -> List[Tuple[int, int, int, str]]`: Returns (element_id, node_i_id, node_j_id, member_type)

**Member Classification**:
- **Column**: Same x and y, different z (vertical member)
- **Girder**: Different x or y, same z (horizontal member)

**Element ID Assignment**:
- Sequential starting from 1
- Assigned in row order from input DataFrame

#### validators.py
Comprehensive validation suite for structural integrity.

**Validation Functions**:
- `validate_axis_parallel()`: All members parallel to x, y, or z axis
- `validate_no_diagonal()`: Girders (horizontal) not diagonal
- `validate_orphan_nodes()`: All nodes connected to at least one element
- `validate_duplicate_ids()`: No duplicate node or element IDs
- `validate_zero_length()`: No zero-length members (node_i == node_j)
- `validate_overlapping()`: No duplicate connections (same start/end nodes)
- `validate_floor_level()`: Z-coordinates represent valid floor levels
- `validate_all()`: Runs all validations in sequence

#### encoding.py
Encoding utilities (part of data pipeline).

#### data_io.py
Input/output operations for data serialization.

### Rust Modules (src/rust/src/)

#### lib.rs - PyO3 Integration Layer
Provides Python bindings to Rust rendering engine via PyO3.

**PyClasses** (exposed to Python):
- `PyNode`: Wraps `graphics::Node` with getters/setters
- `PyElement`: Wraps `graphics::Element`
- `PyRenderData`: Container for nodes and elements with Python-friendly API

**PyFunctions** (exposed to Python):
- `load_and_validate(path: &str) -> PyRenderData`: Loads and parses CSV-like data
- `render_data(data: &PyRenderData) -> PyResult<()>`: Launches eframe UI with render data

**eframe Integration**:
- `AssyPlanApp`: Implements `eframe::App` trait
- `run()`: Launches native window (1200x800) with egui context
- Viewport builder pattern: `ViewportBuilder::default().with_inner_size([...]).with_title(...)`

**Key Pattern**:
- PyO3 wraps Rust types with Python-compatible interfaces
- Conversion between Rust types and Python types via `impl From` traits
- GIL (Global Interpreter Lock) released during rendering

#### main.rs
Entry point for standalone Rust execution.
- Calls `lib::run()` to launch rendering UI directly

#### graphics/renderer.rs
Core 2D rendering engine using egui::Painter.

**Core Types**:
- `Node`: Stores (id, x, y, z)
- `Element`: Stores (id, node_i_id, node_j_id, member_type)
- `RenderData`: Container with scale and offset for transformation

**Key Functions**:
- `calculate_transform(rect: egui::Rect)`: Computes optimal scale and offset to fit all nodes in viewport
- `project_to_2d(x, y, z) -> egui::Pos2`: Projects 3D coordinates to 2D screen space
- `render(painter, rect)`: Main rendering dispatcher

**Rendering Pipeline**:
1. `render_grid()`: Draws grid lines at min(z) level
2. `render_nodes()`: Draws nodes as blue circles (radius 3px)
3. `render_elements()`: Draws elements as lines
   - Columns: Red (RGB 200, 50, 50)
   - Girders: Green (RGB 50, 150, 50)

**Viewport Transformation**:
- Padding: 50 pixels on all sides
- Scale bounded: 0.01 min, 10.0 max
- Auto-centers data in viewport
- Grid spacing: 1000 units

#### graphics/ui.rs
UI state management and egui widget rendering.

**UiState**:
- `show_id_labels`: Toggle node ID display
- `mode`: "Development" or "Simulation" (Simulation disabled in Phase 1)
- `file_path`: Current loaded file path
- `current_tab`: "Settings", "View", or "Result"
- `status_message`: Status text
- `has_data`: Data loaded flag
- `validation_passed`: Validation state

**UI Layout**:
- **Top Panel**: Header with buttons (Open File, Recalc, Reset, Mode toggle, Show IDs)
- **Left Panel**: Status panel (200px default width)
- **Central Panel**: Tabbed view (Settings, View, Result)

**Key Functions**:
- `render_header()`: File/Recalc/Reset buttons, mode toggle
- `render_status_panel()`: Displays current state
- `render_tabs()`: Tab navigation
- `render_settings_tab()`: Settings view
- `render_view_tab()`: Viewport placeholder
- `render_result_tab()`: Validation results
- `render(ui_state, ctx)`: Orchestrates full layout

#### graphics/mod.rs
Module exports for graphics submodule.

## Data Flow Architecture

```
CSV File
    |
    v
[data_loader.py] --charset_normalizer--> Detect encoding
    |
    v
[pandas] --read_csv--> DataFrame
    |
    v
[Validation] Check required columns
    |
    v
[node_table.py] --extract--> Unique nodes (sorted by x,y,z)
    |
    v
[element_table.py] --map--> Elements with classification
    |
    v
[validators.py] --check--> Structural integrity
    |
    v
[PyO3] --convert--> PyNode, PyElement, PyRenderData
    |
    v
[Rust graphics] --render--> eframe UI
    |
    v
[egui::Painter] --project--> 2D screen coordinates
    |
    v
[eframe window] --display--> Rendered visualization
```

## ID Assignment Strategy

### Node IDs
1. Collect all unique (x, y, z) coordinates from both node_i and node_j columns
2. Sort by (x ascending, y ascending, z ascending)
3. Assign IDs starting from 1 in sorted order
4. Result: Consistent, reproducible numbering independent of input row order

### Element IDs
1. Assign sequentially from input DataFrame row order
2. Starting from 1 (not 0)
3. Each row gets element_id = row_index + 1

## Key Design Decisions

### Single-Direction Data Flow
Python processes structural data, Rust renders visualization. No bidirectional communication in Phase 1.

### Axis-Parallel Members Only
Validation enforces members parallel to x, y, or z axis. Diagonal members rejected at validation stage.

### 2D Projection Strategy
Projects 3D structure to 2D plane at minimum z level. Preserves x-y relationships for planar visualization.

### Color-Coded Member Types
- Columns: Red (vertical members)
- Girders: Green (horizontal members)
Enables quick visual identification of structural types.

### Viewport Auto-Scaling
Computes optimal scale to fit all nodes with padding. Prevents clipping and ensures full view of structure.

### Development Mode Only
Phase 1 implements Development mode. Simulation mode UI elements present but disabled/non-functional.

## Integration Patterns

### PyO3 Extension Building
- `Cargo.toml` specifies `crate-type = ["cdylib", "rlib"]`
- `pyproject.toml` configures maturin build backend
- Python module name: `assyplan._core`
- Python 3.12+ required

### Type Conversion Pattern
Rust types implement `impl From<RustType> for PyType` for seamless conversion:
```rust
impl From<graphics::Node> for PyNode { ... }
impl From<graphics::Element> for PyElement { ... }
impl From<graphics::RenderData> for PyRenderData { ... }
```

### Viewport Builder Pattern (eframe 0.27)
```rust
let options = eframe::NativeOptions {
    viewport: egui::ViewportBuilder::default()
        .with_inner_size([1200.0, 800.0])
        .with_title("AssyPlan"),
    ..Default::default()
};
eframe::run_native("AppName", options, Box::new(|_cc| Box::new(App::default())))
```

## Testing Approach

### Python Tests
- pytest 8.0+ for unit tests
- Located in `tests/` directory
- Tests cover data loading, node extraction, element classification, validation

### Rust Tests
- `#[cfg(test)]` modules in each file
- Test core data structures and rendering calculations
- Example: test node creation, element addition, transform calculations

## Known Gotchas and Workarounds

### charset_normalizer Encoding Names
- Returns encoding with underscores: `"utf_8"`, `"euc_kr"`
- NOT dashes: `"utf-8"`, `"euc-kr"`
- pandas.read_csv() handles both formats, no workaround needed

### eframe 0.27 Viewport API
- Requires `ViewportBuilder::default().with_inner_size(...).with_title(...)` pattern
- Cannot use older `NativeOptions::inner_size` API
- Title must be set on ViewportBuilder, not NativeOptions

### PyO3 GIL Management
- eframe rendering releases GIL automatically during window event loop
- No manual GIL release code needed
- Avoid long-running Rust computations holding GIL

### 2D Projection Artifacts
- Orthographic projection to z=min level
- Vertical members may overlap or appear ambiguous
- Grid helps disambiguate z-coordinate relationships

### Member Classification Edge Case
- Members with both x and y different but same z classified as Girder
- Diagonal horizontal members caught by `validate_no_diagonal()`
- These fail validation - not rendered

## Development Workflow

### Python Development
1. Edit `src/python/*.py` modules
2. Run tests: `pytest tests/`
3. No rebuild needed for Python-only changes

### Rust Development
1. Edit `src/rust/src/*.rs`
2. Rebuild: `maturin develop` (dev build) or `cargo build --release`
3. Requires Python 3.12 environment

### Full Pipeline Testing
1. Load CSV: `data_loader.load_csv("data.txt")`
2. Extract nodes: `node_table.create_node_table(df)`
3. Create elements: `element_table.create_element_table(df, nodes)`
4. Validate: `validators.validate_all(nodes, elements)`
5. Render: `assyplan.render_data(render_data_object)`

## Project Structure Summary

```
assyplan/
├── src/
│   ├── python/
│   │   ├── __init__.py
│   │   ├── data_loader.py      # CSV loading + encoding detection
│   │   ├── node_table.py        # Node extraction + ID assignment
│   │   ├── element_table.py     # Element creation + classification
│   │   ├── validators.py        # Structural validation suite
│   │   ├── encoding.py          # Encoding utilities
│   │   └── data_io.py           # Serialization
│   └── rust/
│       ├── Cargo.toml           # Rust dependencies
│       └── src/
│           ├── lib.rs           # PyO3 bindings + eframe app
│           ├── main.rs          # Standalone entry point
│           └── graphics/
│               ├── mod.rs       # Module exports
│               ├── renderer.rs  # 2D rendering engine
│               └── ui.rs        # UI state + egui widgets
├── pyproject.toml               # Python build config + dependencies
├── tests/                       # pytest test suite
└── data.txt                     # Sample input data
```

## Next Steps / Future Phases

- Phase 2: 3D visualization with z-level navigation
- Phase 3: Simulation mode with analysis results
- Phase 4: Interactive editing and member modification
- Phase 5: Multi-project management and history
