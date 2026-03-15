"""Metrics & measures module for construction progress tracking.

This module provides functions to calculate construction metrics including:
- Installed member counts (columns, girders, total)
- Floor-level installation percentages
- Progress tracking per floor level

Reference: devplandoc.md:246-258
"""

from typing import Any, Dict, List, Set, Tuple

from src.python.stability_validators import (
    get_column_elements,
    get_column_floor,
    get_girder_elements,
    get_node_coords,
)


def count_installed_members(
    installed_elements: List[Tuple[int, int, int, str]],
) -> Dict[str, int]:
    """Count installed members by type.

    Reference: devplandoc.md:256-258

    Args:
        installed_elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.

    Returns:
        Dictionary with keys:
            - "columns": Count of installed columns
            - "girders": Count of installed girders
            - "total": Total installed members
    """
    columns = get_column_elements(installed_elements)
    girders = get_girder_elements(installed_elements)

    return {
        "columns": len(columns),
        "girders": len(girders),
        "total": len(installed_elements),
    }


def count_total_members(
    all_elements: List[Tuple[int, int, int, str]],
) -> Dict[str, int]:
    """Count total members in the model by type.

    Reference: devplandoc.md:258

    Args:
        all_elements: List of all (element_id, node_i_id, node_j_id, member_type) tuples.

    Returns:
        Dictionary with keys:
            - "columns": Total columns in model
            - "girders": Total girders in model
            - "total": Total members in model
    """
    return count_installed_members(all_elements)


def get_floor_column_counts(
    elements: List[Tuple[int, int, int, str]],
    nodes: List[Tuple[int, float, float, float]],
) -> Dict[int, int]:
    """Count columns per floor level.

    Floor level is determined by the column's node_i (bottom node) z-coordinate.

    Args:
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.
        nodes: List of (node_id, x, y, z) tuples.

    Returns:
        Dictionary mapping floor level (1-indexed) to column count.
    """
    columns = get_column_elements(elements)
    floor_counts: Dict[int, int] = {}

    for column in columns:
        try:
            floor = get_column_floor(column, nodes)
            floor_counts[floor] = floor_counts.get(floor, 0) + 1
        except (KeyError, ValueError):
            # Skip columns with invalid node references
            continue

    return floor_counts


def calculate_floor_installation_percentage(
    installed_elements: List[Tuple[int, int, int, str]],
    all_elements: List[Tuple[int, int, int, str]],
    nodes: List[Tuple[int, float, float, float]],
) -> Dict[int, float]:
    """Calculate column installation percentage per floor.

    Reference: devplandoc.md:248-254 (Floor-level Column Installation Constraint)

    Args:
        installed_elements: List of currently installed element tuples.
        all_elements: List of all elements in the model.
        nodes: List of (node_id, x, y, z) tuples.

    Returns:
        Dictionary mapping floor level (1-indexed) to installation percentage (0-100).
    """
    # Get total columns per floor
    total_per_floor = get_floor_column_counts(all_elements, nodes)

    # Get installed columns per floor
    installed_per_floor = get_floor_column_counts(installed_elements, nodes)

    # Calculate percentage
    percentages: Dict[int, float] = {}
    for floor, total in total_per_floor.items():
        if total > 0:
            installed = installed_per_floor.get(floor, 0)
            percentages[floor] = (installed / total) * 100.0
        else:
            percentages[floor] = 0.0

    return percentages


def check_floor_installation_constraint(
    target_floor: int,
    installed_elements: List[Tuple[int, int, int, str]],
    all_elements: List[Tuple[int, int, int, str]],
    nodes: List[Tuple[int, float, float, float]],
    threshold_percentage: float = 80.0,
) -> Tuple[bool, float]:
    """Check if installation on a target floor is allowed based on lower floor progress.

    Reference: devplandoc.md:248-254

    The rule: Floor N (N > 1) installation cannot start until floor N-1
    has reached the threshold percentage of column installation.

    Args:
        target_floor: The floor level where new columns would be installed.
        installed_elements: List of currently installed element tuples.
        all_elements: List of all elements in the model.
        nodes: List of (node_id, x, y, z) tuples.
        threshold_percentage: Required percentage of lower floor completion (default: 80%).

    Returns:
        Tuple of:
            - bool: True if installation is allowed, False otherwise
            - float: Current percentage of lower floor (N-1) installation
    """
    # Floor 1 can always install (no lower floor constraint)
    if target_floor <= 1:
        return True, 100.0

    # Get installation percentages
    percentages = calculate_floor_installation_percentage(
        installed_elements, all_elements, nodes
    )

    # Check lower floor (N-1) percentage
    lower_floor = target_floor - 1
    lower_floor_percentage = percentages.get(lower_floor, 0.0)

    # Installation allowed if lower floor meets threshold
    allowed = lower_floor_percentage >= threshold_percentage

    return allowed, lower_floor_percentage


def get_overall_progress(
    installed_elements: List[Tuple[int, int, int, str]],
    all_elements: List[Tuple[int, int, int, str]],
) -> Dict[str, float]:
    """Calculate overall construction progress percentages.

    Args:
        installed_elements: List of currently installed element tuples.
        all_elements: List of all elements in the model.

    Returns:
        Dictionary with keys:
            - "columns_pct": Column installation percentage
            - "girders_pct": Girder installation percentage
            - "total_pct": Total installation percentage
    """
    installed = count_installed_members(installed_elements)
    total = count_total_members(all_elements)

    def safe_pct(num: int, denom: int) -> float:
        return (num / denom * 100.0) if denom > 0 else 0.0

    return {
        "columns_pct": safe_pct(installed["columns"], total["columns"]),
        "girders_pct": safe_pct(installed["girders"], total["girders"]),
        "total_pct": safe_pct(installed["total"], total["total"]),
    }


def get_metrics_summary(
    installed_elements: List[Tuple[int, int, int, str]],
    all_elements: List[Tuple[int, int, int, str]],
    nodes: List[Tuple[int, float, float, float]],
) -> Dict[str, Any]:
    """Get comprehensive metrics summary for current construction state.

    Args:
        installed_elements: List of currently installed element tuples.
        all_elements: List of all elements in the model.
        nodes: List of (node_id, x, y, z) tuples.

    Returns:
        Dictionary with comprehensive metrics:
            - "installed": Installed member counts
            - "total": Total member counts in model
            - "progress": Overall progress percentages
            - "floor_percentages": Installation percentage per floor
    """
    return {
        "installed": count_installed_members(installed_elements),
        "total": count_total_members(all_elements),
        "progress": get_overall_progress(installed_elements, all_elements),
        "floor_percentages": calculate_floor_installation_percentage(
            installed_elements, all_elements, nodes
        ),
    }


def calculate_step_statistics(
    step_table: List[Tuple[int, int, str]],
    elements: List[Tuple[int, int, int, str]],
    nodes: List[Tuple[int, float, float, float]],
) -> Dict[str, Any]:
    """Calculate statistics for each construction step.

    Computes per-step metrics:
    - Floor-level column installation percentage
    - Elements installed in each step
    - Cumulative elements installed

    Args:
        step_table: List of (workfront_id, step, member_id) tuples.
        elements: List of all (element_id, node_i_id, node_j_id, member_type) tuples.
        nodes: List of (node_id, x, y, z) tuples.

    Returns:
        Dictionary with step-by-step statistics:
            - "steps": Dict mapping step_num to step details
            - "total_elements": Total elements in model
            - "total_columns": Total columns in model
            - "total_girders": Total girders in model
    """
    # Build member_id to element mapping
    # member_id is alphabetic (A, B, C, ...), element_id is numeric (1, 2, 3, ...)
    sorted_elements = sorted(elements, key=lambda e: e[0])
    member_to_element: Dict[str, Tuple[int, int, int, str]] = {}

    def get_member_id(idx: int) -> str:
        """Convert index to member ID (A=0, B=1, ..., Z=25, AA=26, etc)."""
        result = ""
        idx += 1
        while idx > 0:
            idx -= 1
            result = chr(ord("A") + (idx % 26)) + result
            idx //= 26
        return result

    for idx, elem in enumerate(sorted_elements):
        member_id = get_member_id(idx)
        member_to_element[member_id] = elem

    # Group step_table by step number
    step_groups: Dict[int, List[str]] = {}
    for workfront_id, step_num, member_id in step_table:
        if step_num not in step_groups:
            step_groups[step_num] = []
        step_groups[step_num].append(member_id)

    # Calculate statistics per step
    step_stats: Dict[int, Dict[str, any]] = {}
    cumulative_elements: List[Tuple[int, int, int, str]] = []
    cumulative_count = 0

    for step_num in sorted(step_groups.keys()):
        members = step_groups[step_num]
        step_elements: List[Tuple[int, int, int, str]] = []

        for member_id in members:
            if member_id in member_to_element:
                step_elements.append(member_to_element[member_id])

        # Count by type for this step
        step_columns = [e for e in step_elements if e[3] == "Column"]
        step_girders = [e for e in step_elements if e[3] == "Girder"]

        # Add to cumulative
        cumulative_elements.extend(step_elements)
        cumulative_count += len(step_elements)

        # Calculate floor percentages after this step
        floor_percentages = calculate_floor_installation_percentage(
            cumulative_elements, elements, nodes
        )

        # Get floor-level counts for this step's columns
        step_floor_columns: Dict[int, int] = {}
        for col in step_columns:
            try:
                floor = get_column_floor(col, nodes)
                step_floor_columns[floor] = step_floor_columns.get(floor, 0) + 1
            except (KeyError, ValueError):
                continue

        step_stats[step_num] = {
            "step_columns": len(step_columns),
            "step_girders": len(step_girders),
            "step_total": len(step_elements),
            "step_floor_columns": step_floor_columns,
            "cumulative_columns": len(
                [e for e in cumulative_elements if e[3] == "Column"]
            ),
            "cumulative_girders": len(
                [e for e in cumulative_elements if e[3] == "Girder"]
            ),
            "cumulative_total": cumulative_count,
            "floor_percentages": floor_percentages,
        }

    # Total counts
    total_columns = len(get_column_elements(elements))
    total_girders = len(get_girder_elements(elements))

    return {
        "steps": step_stats,
        "total_elements": len(elements),
        "total_columns": total_columns,
        "total_girders": total_girders,
    }
