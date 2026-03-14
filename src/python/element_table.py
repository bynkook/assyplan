"""Element table module for structural analysis."""

import pandas as pd
from typing import List, Tuple


def create_element_table(
    df: pd.DataFrame,
    nodes: List[Tuple[int, float, float, float]],
) -> List[Tuple[int, int, int, str]]:
    """Create element table from DataFrame and node table.

    Maps each member (부재) to element_id and resolves node coordinates
    to node IDs. Classifies members as Column (vertical) or Girder (horizontal).

    Args:
        df: DataFrame with 부재ID and node coordinates (node_i_x, node_i_y,
            node_i_z, node_j_x, node_j_y, node_j_z).
        nodes: List of (node_id, x, y, z) tuples from create_node_table.

    Returns:
        List of tuples: (element_id, node_i_id, node_j_id, member_type)
        where member_type is "Column" or "Girder".

    Example:
        >>> df = pd.DataFrame({
        ...     '부재ID': [1, 2],
        ...     'node_i_x': [0.0, 0.0], 'node_i_y': [0.0, 0.0], 'node_i_z': [0.0, 3.0],
        ...     'node_j_x': [0.0, 3.0], 'node_j_y': [0.0, 0.0], 'node_j_z': [3.0, 3.0]
        ... })
        >>> nodes = [(1, 0.0, 0.0, 0.0), (2, 0.0, 0.0, 3.0), (3, 3.0, 0.0, 3.0)]
        >>> result = create_element_table(df, nodes)
        >>> result
        [(1, 1, 2, 'Column'), (2, 2, 3, 'Girder')]
    """
    # Create coordinate to node_id mapping
    coord_to_id: dict[tuple[float, float, float], int] = {
        (x, y, z): nid for nid, x, y, z in nodes
    }

    elements: List[Tuple[int, int, int, str]] = []

    for idx, row in df.iterrows():
        # Extract node coordinates from DataFrame
        node_i_coord = (row["node_i_x"], row["node_i_y"], row["node_i_z"])
        node_j_coord = (row["node_j_x"], row["node_j_y"], row["node_j_z"])

        # Resolve coordinates to node IDs
        node_i_id = coord_to_id[node_i_coord]
        node_j_id = coord_to_id[node_j_coord]

        # Classify member type: Column (vertical) vs Girder (horizontal)
        # Column: same x and y coordinates, different z (vertical member)
        # Girder: different x or y coordinates, same z (horizontal member)
        if node_i_coord[0] == node_j_coord[0] and node_i_coord[1] == node_j_coord[1]:
            member_type = "Column"
        else:
            member_type = "Girder"

        # Assign element_id starting from 1
        element_id = idx + 1
        elements.append((element_id, node_i_id, node_j_id, member_type))

    return elements
