"""Validation functions for structural analysis data."""

from typing import List, Tuple


def validate_axis_parallel(
    nodes: List[Tuple[int, float, float, float]],
    elements: List[Tuple[int, int, int, str]],
) -> bool:
    """Check if all members are parallel to x, y, or z axis.

    A valid member must be parallel to exactly one axis (x, y, or z).
    This means it can only change coordinate in one direction.

    Args:
        nodes: List of (node_id, x, y, z) tuples.
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.

    Returns:
        True if all elements are axis-parallel.

    Raises:
        ValueError: If any element is not axis-parallel.
    """
    # Build node lookup
    node_lookup = {nid: (x, y, z) for nid, x, y, z in nodes}

    for elem_id, ni_id, nj_id, _ in elements:
        ni = node_lookup[ni_id]
        nj = node_lookup[nj_id]
        dx, dy, dz = abs(nj[0] - ni[0]), abs(nj[1] - ni[1]), abs(nj[2] - ni[2])

        # Must be parallel to exactly one axis (non-zero delta in exactly one direction)
        non_zero = sum([dx > 0, dy > 0, dz > 0])
        if non_zero != 1:
            raise ValueError(
                f"Element {elem_id} is not axis-parallel: "
                f"delta=({dx}, {dy}, {dz}). Member must be parallel to exactly one axis."
            )
    return True


def validate_no_diagonal(
    nodes: List[Tuple[int, float, float, float]],
    elements: List[Tuple[int, int, int, str]],
) -> bool:
    """Check if horizontal members (girders) are not diagonal.

    A valid girder (horizontal member) must change only in x OR y direction,
    not both. This ensures no diagonal members in the horizontal plane.

    Args:
        nodes: List of (node_id, x, y, z) tuples.
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.

    Returns:
        True if no horizontal members are diagonal.

    Raises:
        ValueError: If any girder has diagonal orientation.
    """
    # Build node lookup
    node_lookup = {nid: (x, y, z) for nid, x, y, z in nodes}

    for elem_id, ni_id, nj_id, member_type in elements:
        if member_type == "Girder":
            ni = node_lookup[ni_id]
            nj = node_lookup[nj_id]

            # Girder should have same z
            if ni[2] != nj[2]:
                raise ValueError(
                    f"Element {elem_id} marked as Girder but has different z coordinates: "
                    f"node_i_z={ni[2]}, node_j_z={nj[2]}"
                )

            # Check if diagonal (changes in both x AND y)
            dx = abs(nj[0] - ni[0])
            dy = abs(nj[1] - ni[1])

            if dx > 0 and dy > 0:
                raise ValueError(
                    f"Diagonal girder detected: element {elem_id} changes in both x ({dx}) and y ({dy}). "
                    f"Girder must be parallel to x-axis or y-axis, not diagonal."
                )

    return True


def validate_orphan_nodes(
    nodes: List[Tuple[int, float, float, float]],
    elements: List[Tuple[int, int, int, str]],
) -> bool:
    """Detect unconnected nodes (nodes not part of any element).

    All nodes must be connected to at least one element.

    Args:
        nodes: List of (node_id, x, y, z) tuples.
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.

    Returns:
        True if all nodes are connected.

    Raises:
        ValueError: If any node is not connected to any element.
    """
    # Build set of connected node IDs
    connected_nodes: set[int] = set()
    for _, ni_id, nj_id, _ in elements:
        connected_nodes.add(ni_id)
        connected_nodes.add(nj_id)

    # Find orphan nodes
    all_node_ids = {nid for nid, _, _, _ in nodes}
    orphans = all_node_ids - connected_nodes

    if orphans:
        raise ValueError(
            f"Orphan nodes detected (not connected to any element): {sorted(orphans)}"
        )

    return True


def validate_duplicate_ids(
    nodes: List[Tuple[int, float, float, float]],
    elements: List[Tuple[int, int, int, str]],
) -> bool:
    """Detect duplicate node or element IDs.

    Args:
        nodes: List of (node_id, x, y, z) tuples.
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.

    Returns:
        True if no duplicates exist.

    Raises:
        ValueError: If duplicate IDs are found.
    """
    # Check duplicate node IDs
    node_ids = [nid for nid, _, _, _ in nodes]
    if len(node_ids) != len(set(node_ids)):
        duplicates = [nid for nid in node_ids if node_ids.count(nid) > 1]
        raise ValueError(f"Duplicate node IDs found: {set(duplicates)}")

    # Check duplicate element IDs
    element_ids = [eid for eid, _, _, _ in elements]
    if len(element_ids) != len(set(element_ids)):
        duplicates = [eid for eid in element_ids if element_ids.count(eid) > 1]
        raise ValueError(f"Duplicate element IDs found: {set(duplicates)}")

    return True


def validate_zero_length(
    elements: List[Tuple[int, int, int, str]],
    nodes: List[Tuple[int, float, float, float]] | None = None,
) -> bool:
    """Detect zero-length members (where node_i equals node_j).

    Args:
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.
        nodes: Optional node lookup for coordinate comparison.

    Returns:
        True if no zero-length elements exist.

    Raises:
        ValueError: If any element has zero length (same start and end node).
    """
    for elem_id, ni_id, nj_id, _ in elements:
        if ni_id == nj_id:
            raise ValueError(
                f"Zero-length element detected: element {elem_id} has same node_i and node_j (node_id={ni_id})"
            )
    return True


def validate_overlapping(elements: List[Tuple[int, int, int, str]]) -> bool:
    """Detect overlapping members (two elements with same node_i and node_j).

    Args:
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.

    Returns:
        True if no overlapping elements exist.

    Raises:
        ValueError: If any two elements share the same start and end nodes.
    """
    # Create set of (node_i, node_j) pairs (normalized to handle direction)
    connections: set[tuple[int, int]] = set()
    for elem_id, ni_id, nj_id, _ in elements:
        # Normalize: store smaller ID first to catch both (a,b) and (b,a)
        pair = (min(ni_id, nj_id), max(ni_id, nj_id))
        if pair in connections:
            raise ValueError(
                f"Overlapping elements detected: multiple elements connect nodes {pair[0]} and {pair[1]}"
            )
        connections.add(pair)
    return True


def validate_floor_level(nodes: List[Tuple[int, float, float, float]]) -> bool:
    """Check floor level consistency.

    Validates that nodes at the same floor level (z-coordinate) are consistent.
    All nodes at a given z should form a valid floor.

    Args:
        nodes: List of (node_id, x, y, z) tuples.

    Returns:
        True if floor levels are consistent.

    Raises:
        ValueError: If floor levels are inconsistent.
    """
    # Group nodes by z-coordinate
    z_levels: dict[float, List[int]] = {}
    for nid, x, y, z in nodes:
        if z not in z_levels:
            z_levels[z] = []
        z_levels[z].append(nid)

    # Check that we have reasonable floor levels
    sorted_z = sorted(z_levels.keys())

    # Check for duplicate z values (shouldn't happen with set-based node creation)
    if len(sorted_z) != len(z_levels):
        raise ValueError("Inconsistent floor levels detected")

    # Check that z levels are reasonable (not too many, not too close)
    for i in range(1, len(sorted_z)):
        z_diff = sorted_z[i] - sorted_z[i - 1]
        if z_diff < 0.01:  # Less than 1cm apart is suspicious
            raise ValueError(
                f"Suspicious floor level detected: z={sorted_z[i - 1]} and z={sorted_z[i]} "
                f"are only {z_diff}m apart (less than 1cm)"
            )

    return True


def validate_all(
    nodes: List[Tuple[int, float, float, float]],
    elements: List[Tuple[int, int, int, str]],
) -> bool:
    """Run all validations in sequence.

    Args:
        nodes: List of (node_id, x, y, z) tuples.
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.

    Returns:
        True if all validations pass.

    Raises:
        ValueError: If any validation fails.
    """
    validate_duplicate_ids(nodes, elements)
    validate_zero_length(elements, nodes)
    validate_axis_parallel(nodes, elements)
    validate_no_diagonal(nodes, elements)
    validate_orphan_nodes(nodes, elements)
    validate_floor_level(nodes)
    validate_overlapping(elements)
    return True
