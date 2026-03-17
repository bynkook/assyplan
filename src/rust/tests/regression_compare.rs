use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use assyplan::graphics::ui::SimWorkfront;
use assyplan::sim_engine::run_all_scenarios;
use assyplan::sim_grid::SimGrid;
use assyplan::stability::{build_step_elements_map, generate_all_tables, get_floor_column_data};

// Sim fingerprint changed after bootstrap anchor-priority update
// (first-cycle candidate selection prefers each workfront's anchor column).
const EXPECTED_SIM_FINGERPRINT_V2: u64 = 428175403154935975;
// Dev fingerprint changed after canonical Dev step generation update
// (local-step completion + global-cycle merge semantics).
const EXPECTED_DEV_FINGERPRINT_V2: u64 = 7561072595717411788;

fn hash_u64(input: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    hasher.finish()
}

fn sim_fingerprint() -> u64 {
    let grid = SimGrid::new(4, 4, 3, 6000.0, 6000.0, 4000.0);
    let workfronts = vec![
        SimWorkfront {
            id: 1,
            grid_x: 0,
            grid_y: 0,
        },
        SimWorkfront {
            id: 2,
            grid_x: 3,
            grid_y: 3,
        },
    ];

    let scenarios = run_all_scenarios(4, &grid, &workfronts, (0.5, 0.3, 0.2), 0.3);

    let mut buf = String::new();
    for scenario in &scenarios {
        buf.push_str(&format!(
            "S:{}:{}:{}:{}:{};",
            scenario.id,
            scenario.metrics.total_steps,
            scenario.metrics.total_members_installed,
            scenario
                .steps
                .iter()
                .flat_map(|step| step.sequences.iter().map(|seq| seq.sequence_number))
                .max()
                .unwrap_or(0),
            format!("{:?}", scenario.metrics.termination_reason)
        ));

        for step in &scenario.steps {
            buf.push_str(&format!(
                "T:{}:{}:{}:{};",
                step.workfront_id,
                step.floor,
                step.pattern,
                step.element_ids.len()
            ));
            for seq in &step.sequences {
                buf.push_str(&format!("Q:{}:{};", seq.sequence_number, seq.element_id));
            }
        }
    }

    hash_u64(&buf)
}

fn dev_fingerprint() -> u64 {
    let grid = SimGrid::new(3, 3, 3, 6000.0, 6000.0, 4000.0);
    let nodes = grid.nodes.clone();
    let elements = grid.elements.clone();

    let mut element_data = Vec::new();
    for (idx, element) in elements.iter().enumerate() {
        let member_id = format!("E{}", element.id);
        let predecessor = if idx == 0 {
            None
        } else {
            Some(format!("E{}", elements[idx - 1].id))
        };
        element_data.push((member_id, predecessor));
    }

    let result = generate_all_tables(&nodes, &elements, &element_data);
    let step_elements = build_step_elements_map(&result.step_table, &elements, &nodes);
    let floor_data = get_floor_column_data(&elements, &nodes);

    let mut buf = String::new();
    buf.push_str(&format!(
        "R:{}:{}:{}:{}:{};",
        result.sequence_table.len(),
        result.step_table.len(),
        result.max_step,
        result.workfront_count,
        result.errors.len()
    ));

    for seq in &result.sequence_table {
        buf.push_str(&format!(
            "SEQ:{}:{}:{}:{};",
            seq.sequence_order, seq.workfront_id, seq.element_id, seq.member_type
        ));
    }

    for step in &result.step_table {
        buf.push_str(&format!("STEP:{}:{};", step.workfront_id, step.step));
        for eid in &step.element_ids {
            buf.push_str(&format!("E:{};", eid));
        }
    }

    for (floor, count) in floor_data {
        buf.push_str(&format!("F:{}:{};", floor, count));
    }

    for (step_idx, list) in step_elements.iter().enumerate() {
        buf.push_str(&format!("M:{}:{};", step_idx, list.len()));
        for (eid, mtype, floor) in list {
            buf.push_str(&format!("D:{}:{}:{};", eid, mtype, floor));
        }
    }

    hash_u64(&buf)
}

#[test]
fn regression_guard_sim_fingerprint_v2() {
    let actual = sim_fingerprint();
    assert_eq!(
        actual, EXPECTED_SIM_FINGERPRINT_V2,
        "SIM fingerprint mismatch. expected={}, actual={}",
        EXPECTED_SIM_FINGERPRINT_V2, actual
    );
}

#[test]
fn regression_guard_dev_fingerprint_v2() {
    let actual = dev_fingerprint();
    assert_eq!(
        actual, EXPECTED_DEV_FINGERPRINT_V2,
        "DEV fingerprint mismatch. expected={}, actual={}",
        EXPECTED_DEV_FINGERPRINT_V2, actual
    );
}
