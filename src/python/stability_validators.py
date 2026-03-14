"""Stability validation functions for structural member assembly.

This module provides validation functions to ensure structural stability
during member assembly operations.
"""

from typing import List, Set, Tuple


def get_node_coords(
    node_id: int,
    nodes: List[Tuple[int, float, float, float]],
) -> Tuple[float, float, float]:
    """Retrieve the coordinates of a node by its ID.

    Args:
        node_id: The ID of the node to look up.
        nodes: List of (node_id, x, y, z) tuples.

    Returns:
        Tuple of (x, y, z) coordinates.

    Raises:
        KeyError: If the node_id is not found in nodes.
    """
    for nid, x, y, z in nodes:
        if nid == node_id:
            return (x, y, z)
    raise KeyError(f"Node with ID {node_id} not found in nodes list")


def get_elements_at_node(
    node_id: int,
    elements: List[Tuple[int, int, int, str]],
) -> List[Tuple[int, int, int, str]]:
    """Get all elements connected to a specific node.

    Args:
        node_id: The ID of the node to find connected elements for.
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.

    Returns:
        List of element tuples that are connected to the given node.
    """
    connected: List[Tuple[int, int, int, str]] = []
    for elem in elements:
        elem_id, ni_id, nj_id, member_type = elem
        if ni_id == node_id or nj_id == node_id:
            connected.append(elem)
    return connected


def get_column_elements(
    elements: List[Tuple[int, int, int, str]],
) -> List[Tuple[int, int, int, str]]:
    """Get all column elements from the elements list.

    Args:
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.

    Returns:
        List of column elements only.
    """
    return [e for e in elements if e[3] == "Column"]


def get_girder_elements(
    elements: List[Tuple[int, int, int, str]],
) -> List[Tuple[int, int, int, str]]:
    """Get all girder elements from the elements list.

    Args:
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.

    Returns:
        List of girder elements only.
    """
    return [e for e in elements if e[3] == "Girder"]


def validate_minimum_assembly(
    nodes: List[Tuple[int, float, float, float]],
    elements: List[Tuple[int, int, int, str]],
) -> bool:
    """Check if elements form a valid minimum assembly unit.

    A minimum assembly unit consists of:
    - 3 adjacent columns
    - 2 girders connecting their j-nodes at 90 degree angles

    This represents the minimum stable independent structure that can
    exist without connection to other members.

    Args:
        nodes: List of (node_id, x, y, z) tuples.
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.

    Returns:
        True if valid minimum assembly unit.

    Raises:
        ValueError: If the elements do not form a valid minimum assembly unit.
    """
    columns = get_column_elements(elements)
    girders = get_girder_elements(elements)

    # Must have exactly 3 columns and 2 girders
    if len(columns) != 3:
        raise ValueError(
            f"Minimum assembly requires exactly 3 columns, found {len(columns)}"
        )

    if len(girders) != 2:
        raise ValueError(
            f"Minimum assembly requires exactly 2 girders, found {len(girders)}"
        )

    # Build node lookup
    node_lookup = {nid: (x, y, z) for nid, x, y, z in nodes}

    # Get j-nodes of all columns (top nodes - columns go from bottom to top)
    # For columns, node_i is bottom, node_j is top
    column_j_nodes: Set[int] = set()
    column_nodes: Set[int] = set()
    for _, ni_id, nj_id, _ in columns:
        column_j_nodes.add(nj_id)
        column_nodes.add(ni_id)
        column_nodes.add(nj_id)

    # Get nodes of girders
    girder_nodes: Set[int] = set()
    for _, ni_id, nj_id, _ in girders:
        girder_nodes.add(ni_id)
        girder_nodes.add(nj_id)

    # Girders must connect the j-nodes of columns
    if not girder_nodes.issubset(column_j_nodes):
        missing = girder_nodes - column_j_nodes
        raise ValueError(
            f"Girders must connect column j-nodes only. "
            f"Non-column j-nodes found: {missing}"
        )

    # Check 90-degree angle between girders
    # Get coordinates of girder nodes
    girder_coords = [node_lookup[nid] for nid in girder_nodes]
    if len(girder_coords) < 2:
        raise ValueError("Not enough girder nodes to form 90-degree angle")

    # Find the two girders and check their orientation
    girder1 = girders[0]
    girder2 = girders[1]

    g1_ni = node_lookup[girder1[1]]
    g1_nj = node_lookup[girder1[2]]
    g2_ni = node_lookup[girder2[1]]
    g2_nj = node_lookup[girder2[2]]

    # Calculate directions
    g1_dir = (g1_nj[0] - g1_ni[0], g1_nj[1] - g1_ni[1])
    g2_dir = (g2_nj[0] - g2_ni[0], g2_nj[1] - g2_ni[1])

    # Normalize directions (get sign/direction)
    g1_dir = (
        1 if g1_dir[0] > 0 else -1 if g1_dir[0] < 0 else 0,
        1 if g1_dir[1] > 0 else -1 if g1_dir[1] < 0 else 0,
    )
    g2_dir = (
        1 if g2_dir[0] > 0 else -1 if g2_dir[0] < 0 else 0,
        1 if g2_dir[1] > 0 else -1 if g2_dir[1] < 0 else 0,
    )

    # Check if perpendicular (dot product should be 0 for 90 degrees)
    # For axis-aligned: (1,0) dot (0,1) = 0 or (0,1) dot (1,0) = 0
    dot_product = g1_dir[0] * g2_dir[0] + g1_dir[1] * g2_dir[1]
    if dot_product != 0:
        raise ValueError(
            f"Girders must be at 90 degrees. "
            f"Directions: {g1_dir} and {g2_dir} (dot={dot_product})"
        )

    return True


def validate_column_support(
    column_element: Tuple[int, int, int, str],
    nodes: List[Tuple[int, float, float, float]],
    stable_elements: Set[int],
) -> bool:
    """Check if a column's node_i is properly supported.

    A column's node_i (bottom node) must be either:
    - At ground level (z=0), OR
    - Connected to node_j of an already-stable column below

    Args:
        column_element: Tuple of (element_id, node_i_id, node_j_id, "Column").
        nodes: List of (node_id, x, y, z) tuples.
        stable_elements: Set of element IDs that have passed stability validation.

    Returns:
        True if the column is properly supported, False otherwise.
    """
    elem_id, ni_id, nj_id, member_type = column_element

    if member_type != "Column":
        raise ValueError(f"Element {elem_id} is not a Column")

    # Get coordinates of node_i
    ni_coords = get_node_coords(ni_id, nodes)
    ni_z = ni_coords[2]

    # Check if at ground level
    if ni_z == 0:
        return True

    # If not at ground, check if connected to stable column's node_j
    # Find elements connected to node_i
    connected_elements = get_elements_at_node(
        ni_id, elements=[]
    )  # Will search manually

    # Build element lookup for stable columns
    stable_columns = []
    all_elements = []  # Need full elements list

    for elem in all_elements:
        if elem[0] in stable_elements and elem[3] == "Column":
            stable_columns.append(elem)

    # Check if any stable column has its node_j at same x,y as our node_i
    for col in stable_columns:
        _, col_ni, col_nj, _ = col
        col_nj_coords = get_node_coords(col_nj, nodes)

        # Same x,y position (horizontal alignment)
        if (
            abs(col_nj_coords[0] - ni_coords[0]) < 0.001
            and abs(col_nj_coords[1] - ni_coords[1]) < 0.001
        ):
            # Check if column is below (lower z)
            if col_nj_coords[2] < ni_z:
                return True

    return False


def validate_girder_support(
    girder_element: Tuple[int, int, int, str],
    nodes: List[Tuple[int, float, float, float]],
    elements: List[Tuple[int, int, int, str]],
    stable_elements: Set[int],
) -> bool:
    """Check if a girder's both nodes are supported.

    A girder's node_i and node_j must both be connected to
    already-stable columns or girders.

    Args:
        girder_element: Tuple of (element_id, node_i_id, node_j_id, "Girder").
        nodes: List of (node_id, x, y, z) tuples.
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.
        stable_elements: Set of element IDs that have passed stability validation.

    Returns:
        True if both ends are properly supported, False otherwise.
    """
    elem_id, ni_id, nj_id, member_type = girder_element

    if member_type != "Girder":
        raise ValueError(f"Element {elem_id} is not a Girder")

    # Check support for node_i
    ni_supported = _is_node_supported(ni_id, nodes, elements, stable_elements)

    # Check support for node_j
    nj_supported = _is_node_supported(nj_id, nodes, elements, stable_elements)

    return ni_supported and nj_supported


def _is_node_supported(
    node_id: int,
    nodes: List[Tuple[int, float, float, float]],
    elements: List[Tuple[int, int, int, str]],
    stable_elements: Set[int],
) -> bool:
    """Check if a node is supported by stable elements.

    A node is supported if it's connected to any stable column or girder.

    Args:
        node_id: The ID of the node to check.
        nodes: List of (node_id, x, y, z) tuples.
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.
        stable_elements: Set of element IDs that have passed stability validation.

    Returns:
        True if the node is connected to a stable element, False otherwise.
    """
    connected = get_elements_at_node(node_id, elements)

    for elem in connected:
        elem_id = elem[0]
        if elem_id in stable_elements:
            return True

    return False


def validate_no_ground_girder(
    elements: List[Tuple[int, int, int, str]],
    nodes: List[Tuple[int, float, float, float]],
) -> bool:
    """Check that no girder exists at ground level (z=0).

    According to structural rules, girders cannot exist at z=0.
    Only columns should exist at ground level.

    Args:
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.
        nodes: List of (node_id, x, y, z) tuples.

    Returns:
        True if no girder exists at ground level.

    Raises:
        ValueError: If a girder is found at ground level (z=0).
    """
    node_lookup = {nid: (x, y, z) for nid, x, y, z in nodes}

    for elem_id, ni_id, nj_id, member_type in elements:
        if member_type == "Girder":
            ni_z = node_lookup[ni_id][2]
            nj_z = node_lookup[nj_id][2]

            if ni_z == 0 or nj_z == 0:
                raise ValueError(
                    f"Girder {elem_id} exists at ground level (z=0): "
                    f"node_i_z={ni_z}, node_j_z={nj_z}. "
                    f"Girders cannot exist at z=0."
                )

    return True
