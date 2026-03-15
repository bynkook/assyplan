"""Workfront Step Table module for assigning construction steps based on stability.

This module provides functions to:
- Assign steps to members based on stability validation
- Create step table from sequence, nodes, elements, and DAG
- Save the resulting step table to CSV or JSON
"""

import json
from typing import Any, Dict, List, Set, Tuple

import pandas as pd

from src.python.stability_validators import (
    double_check_cumulative_stability,
    get_column_elements,
    get_column_floor,
    get_girder_elements,
    validate_column_support,
    validate_girder_support,
    validate_minimum_assembly,
)
from src.python.metrics import check_floor_installation_constraint


def _build_member_to_element_mapping(
    elements: List[Tuple[int, int, int, str]],
) -> Dict[str, int]:
    """Build mapping from member ID (string) to element ID (int).

    Assumes elements are indexed sequentially starting from 1,
    and member IDs follow alphabetical ordering that maps to
    element indices.

    Args:
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.

    Returns:
        Dictionary mapping member_id to element_id.
    """
    # Sort elements by element_id to ensure consistent ordering
    sorted_elements = sorted(elements, key=lambda e: e[0])

    # Generate member IDs: A, B, C, ... AA, AB, ...
    def get_member_id(idx: int) -> str:
        """Convert index to member ID (A=0, B=1, ..., Z=25, AA=26, etc)."""
        result = ""
        idx += 1
        while idx > 0:
            idx -= 1
            result = chr(ord("A") + (idx % 26)) + result
            idx //= 26
        return result

    mapping: Dict[str, int] = {}
    for idx, element in enumerate(sorted_elements):
        element_id = element[0]
        member_id = get_member_id(idx)
        mapping[member_id] = element_id

    return mapping


def _get_element_by_id(
    element_id: int,
    elements: List[Tuple[int, int, int, str]],
) -> Tuple[int, int, int, str] | None:
    """Get element tuple by element_id.

    Args:
        element_id: The element ID to look up.
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.

    Returns:
        Element tuple if found, None otherwise.
    """
    for elem in elements:
        if elem[0] == element_id:
            return elem
    return None


def _is_structure_stable(
    elements_to_check: List[Tuple[int, int, int, str]],
    nodes: List[Tuple[int, float, float, float]],
    all_elements: List[Tuple[int, int, int, str]],
    stable_elements: Set[int],
    is_first_step: bool,
) -> bool:
    """Check if adding elements creates a stable structure.

    Args:
        elements_to_check: List of elements to check for stability.
        nodes: List of (node_id, x, y, z) tuples.
        all_elements: Complete list of all elements (for looking up stable element details).
        stable_elements: Set of already stable element IDs.
        is_first_step: Whether this is the first step (requires minimum assembly).

    Returns:
        True if the structure is stable, False otherwise.
    """
    if not elements_to_check:
        return False

    # For first step, validate minimum assembly
    if is_first_step:
        try:
            validate_minimum_assembly(nodes, elements_to_check)
            return True
        except ValueError:
            return False

    # For subsequent steps, check support for each element
    for elem in elements_to_check:
        elem_id, _, _, member_type = elem

        if member_type == "Column":
            # Check column support - needs access to all elements to find stable columns
            if not validate_column_support(elem, nodes, all_elements, stable_elements):
                return False
        elif member_type == "Girder":
            # Check girder support - needs access to all elements to find stable connections
            if not validate_girder_support(elem, nodes, all_elements, stable_elements):
                return False

    return True


def assign_steps(
    sequence: List[Tuple[int, str]],
    nodes: List[Tuple[int, float, float, float]],
    elements: List[Tuple[int, int, int, str]],
    dag: Dict[str, Dict[str, List[str]]],
) -> List[Tuple[int, int, str]]:
    """Assign construction steps to members based on stability validation.

    Processes members in sequence order per workfront. Steps are assigned
    based on stability validation - members that form a stable unit together
    receive the same step. Step starts from 1, not 0.

    Args:
        sequence: List of (workfront_id, member_id) tuples from sequence_table.
        nodes: List of (node_id, x, y, z) tuples.
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.
        dag: Dictionary representing the DAG (unused in current implementation,
            kept for API consistency).

    Returns:
        List of (workfront_id, step, member_id) tuples.
        Step starts from 1. Multiple members can share the same step
        if they form a stable unit together.

    Raises:
        ValueError: If sequence is empty.
        ValueError: If nodes is empty.
        ValueError: If elements is empty.
    """
    if not sequence:
        raise ValueError("Sequence cannot be empty")
    if not nodes:
        raise ValueError("Nodes cannot be empty")
    if not elements:
        raise ValueError("Elements cannot be empty")

    # Build mapping from member_id to element_id
    member_to_element = _build_member_to_element_mapping(elements)

    # Group sequence by workfront
    workfront_sequences: Dict[int, List[str]] = {}
    for workfront_id, member_id in sequence:
        if workfront_id not in workfront_sequences:
            workfront_sequences[workfront_id] = []
        workfront_sequences[workfront_id].append(member_id)

    result: List[Tuple[int, int, str]] = []

    # Process each workfront
    for workfront_id in sorted(workfront_sequences.keys()):
        members = workfront_sequences[workfront_id]

        # Track stable elements for this workfront
        stable_elements: Set[int] = set()
        current_step = 1  # Step starts from 1
        current_step_members: List[str] = []

        for member_id in members:
            # Get element_id for this member
            if member_id not in member_to_element:
                # If member not in mapping, try to find by index
                # This handles cases where member_id might be numeric
                try:
                    idx = int(member_id) - 1
                    if 0 <= idx < len(elements):
                        element_id = elements[idx][0]
                    else:
                        raise KeyError(f"Member {member_id} not found")
                except (ValueError, IndexError):
                    raise KeyError(f"Member {member_id} not found in element mapping")
            else:
                element_id = member_to_element[member_id]

            # Get the element tuple
            element = _get_element_by_id(element_id, elements)
            if element is None:
                raise ValueError(f"Element with ID {element_id} not found")

            # Add element to current step candidates
            current_step_members.append(member_id)

            # Get elements for current step
            current_elements: List[Tuple[int, int, int, str]] = []
            for m_id in current_step_members:
                m_elem_id = member_to_element.get(m_id)
                if m_elem_id:
                    elem = _get_element_by_id(m_elem_id, elements)
                    if elem:
                        current_elements.append(elem)

            # Check if current step forms stable structure
            is_first_step = current_step == 1 and len(stable_elements) == 0
            is_stable = _is_structure_stable(
                current_elements,
                nodes,
                elements,  # Pass full elements list for stable element lookup
                stable_elements,
                is_first_step,
            )

            if is_stable:
                # Assign current step to all members in this group
                for m_id in current_step_members:
                    result.append((workfront_id, current_step, m_id))

                # Update stable_elements with current group
                for m_id in current_step_members:
                    m_elem_id = member_to_element.get(m_id)
                    if m_elem_id:
                        stable_elements.add(m_elem_id)

                # Move to next step
                current_step += 1
                current_step_members = []
            else:
                # Not stable yet - continue accumulating
                # If this is the first member and it's not stable alone,
                # we still need to assign step 1 to it
                if len(current_step_members) == 1 and current_step == 1:
                    # First member alone - assign step 1 anyway
                    # (will be re-evaluated when more members are added)
                    pass

        # Handle any remaining members that didn't form stable structure
        # Assign them to the current step
        if current_step_members:
            for m_id in current_step_members:
                result.append((workfront_id, current_step, m_id))

    return result


def verify_step_table_stability(
    step_table: List[Tuple[int, int, str]],
    nodes: List[Tuple[int, float, float, float]],
    elements: List[Tuple[int, int, int, str]],
    floor_threshold: float = 80.0,
) -> Dict[str, Any]:
    """Verify cumulative stability of step table (double-check gate).

    This function performs the "double check" verification described in
    devplandoc.md:117-119. It validates that:
    1. Each step results in cumulative structural stability
    2. Floor-level installation constraints are satisfied

    Args:
        step_table: List of (workfront_id, step, member_id) tuples.
        nodes: List of (node_id, x, y, z) tuples.
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.
        floor_threshold: Required percentage of lower floor completion (default: 80%).

    Returns:
        Dictionary with verification results:
            - "valid": bool - True if all steps pass verification
            - "step_results": Dict[int, Dict] - Results per step
            - "failed_steps": List[int] - Steps that failed verification
            - "floor_violations": List[Dict] - Floor constraint violations
    """
    # Build member to element mapping
    member_to_element = _build_member_to_element_mapping(elements)

    # Group step_table by step
    steps_by_number: Dict[int, List[Tuple[int, int, str]]] = {}
    for wf_id, step, member_id in step_table:
        if step not in steps_by_number:
            steps_by_number[step] = []
        steps_by_number[step].append((wf_id, step, member_id))

    # Track verification state
    verified_stable: Set[int] = set()
    installed_element_ids: Set[int] = set()
    step_results: Dict[int, Dict] = {}
    failed_steps: List[int] = []
    floor_violations: List[Dict] = []

    # Process steps in order
    for step_num in sorted(steps_by_number.keys()):
        step_entries = steps_by_number[step_num]

        # Get elements for this step
        step_elements: List[Tuple[int, int, int, str]] = []
        for _, _, member_id in step_entries:
            elem_id = member_to_element.get(member_id)
            if elem_id:
                elem = _get_element_by_id(elem_id, elements)
                if elem:
                    step_elements.append(elem)
                    installed_element_ids.add(elem_id)

        # Get all installed elements (filter out None values explicitly)
        installed_elements: List[Tuple[int, int, int, str]] = []
        for eid in installed_element_ids:
            elem = _get_element_by_id(eid, elements)
            if elem is not None:
                installed_elements.append(elem)

        # Check floor constraints for columns in this step
        for elem in step_elements:
            elem_id, _, _, member_type = elem
            if member_type == "Column":
                try:
                    floor = get_column_floor(elem, nodes)
                    allowed, lower_pct = check_floor_installation_constraint(
                        floor,
                        installed_elements,
                        elements,
                        nodes,
                        floor_threshold,
                    )
                    if not allowed:
                        floor_violations.append(
                            {
                                "step": step_num,
                                "element_id": elem_id,
                                "floor": floor,
                                "lower_floor_percentage": lower_pct,
                                "required_threshold": floor_threshold,
                            }
                        )
                except (KeyError, ValueError):
                    pass  # Skip elements with invalid node references

        # Perform double-check: cumulative stability verification
        is_stable, verified_stable, failed_elems = double_check_cumulative_stability(
            installed_elements,
            nodes,
            elements,
            verified_stable,
        )

        step_results[step_num] = {
            "stable": is_stable,
            "elements_verified": len(verified_stable),
            "failed_elements": failed_elems,
        }

        if not is_stable:
            failed_steps.append(step_num)

    # Overall validity
    is_valid = len(failed_steps) == 0 and len(floor_violations) == 0

    return {
        "valid": is_valid,
        "step_results": step_results,
        "failed_steps": failed_steps,
        "floor_violations": floor_violations,
    }


def create_step_table(
    sequence: List[Tuple[int, str]],
    nodes: List[Tuple[int, float, float, float]],
    elements: List[Tuple[int, int, int, str]],
    dag: Dict[str, Dict[str, List[str]]],
) -> List[Tuple[int, int, str]]:
    """Create workfront step table from sequence, nodes, elements, and DAG.

    This is a wrapper function that calls assign_steps and returns
    the step table.

    Args:
        sequence: List of (workfront_id, member_id) tuples from sequence_table.
        nodes: List of (node_id, x, y, z) tuples.
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.
        dag: Dictionary representing the DAG.

    Returns:
        List of (workfront_id, step, member_id) tuples.
        Step starts from 1.

    Raises:
        ValueError: If sequence is empty.
    """
    if not sequence:
        raise ValueError("Sequence cannot be empty")

    return assign_steps(sequence, nodes, elements, dag)


def save_step_table(
    step_table: List[Tuple[int, int, str]],
    filepath: str,
    format: str = "csv",
) -> None:
    """Save workfront step table to file.

    Args:
        step_table: List of (workfront_id, step, member_id) tuples.
        filepath: Output file path.
        format: Output format - "csv" or "json". Defaults to "csv".

    Raises:
        ValueError: If format is not "csv" or "json".
        ValueError: If step_table is empty.
    """
    if not step_table:
        raise ValueError("Step table cannot be empty")

    if format.lower() == "csv":
        df = pd.DataFrame(step_table, columns=["workfront_id", "step", "member_id"])
        df.to_csv(filepath, index=False)
    elif format.lower() == "json":
        data = [
            {"workfront_id": wf_id, "step": step, "member_id": member_id}
            for wf_id, step, member_id in step_table
        ]
        with open(filepath, "w", encoding="utf-8") as f:
            json.dump(data, f, ensure_ascii=False, indent=2)
    else:
        raise ValueError(f"Unsupported format: {format}. Use 'csv' or 'json'.")


if __name__ == "__main__":
    # Simple test cases
    print("Running step table tests...")

    # Test 1: Simple 3 columns + 2 girders (minimum assembly)
    nodes1 = [
        (1, 0.0, 0.0, 0.0),
        (2, 0.0, 0.0, 3.0),
        (3, 1.0, 0.0, 0.0),
        (4, 1.0, 0.0, 3.0),
        (5, 1.0, 1.0, 0.0),
        (6, 1.0, 1.0, 3.0),
    ]
    elements1 = [
        (1, 1, 2, "Column"),
        (2, 3, 4, "Column"),
        (3, 5, 6, "Column"),
        (4, 2, 4, "Girder"),
        (5, 4, 6, "Girder"),
    ]
    sequence1 = [(1, "A"), (1, "B"), (1, "C"), (1, "D"), (1, "E")]
    dag1 = {}

    steps1 = assign_steps(sequence1, nodes1, elements1, dag1)
    print(f"Test 1 (minimum assembly): {steps1}")
    # All should be step 1 (minimum assembly is stable)
    assert all(step == 1 for _, step, _ in steps1)

    # Test 2: Multiple workfronts
    sequence2 = [
        (1, "A"),
        (1, "B"),
        (2, "C"),
        (2, "D"),
    ]
    steps2 = create_step_table(sequence2, nodes1, elements1, dag1)
    print(f"Test 2 (multiple workfronts): {steps2}")

    print("\nAll tests passed!")
