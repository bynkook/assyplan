"""Node table module for extracting unique nodes from element data."""

import pandas as pd
from typing import List, Tuple


def create_node_table(df: pd.DataFrame) -> List[Tuple[int, float, float, float]]:
    """Create node table from DataFrame.

    Extracts unique node coordinates from node_i and node_j columns,
    sorts by z → x → y (ascending), and assigns IDs starting from 1.

    Args:
        df: DataFrame with node_i_x/y/z and node_j_x/y/z columns

    Returns:
        List of tuples: (node_id, x, y, z)

    Example:
        >>> df = load_csv('data.txt')
        >>> nodes = create_node_table(df)
        >>> print(nodes[0])  # (1, 0.0, 0.0, 0.0)
    """
    # Collect all unique coordinates
    coords = set()
    for _, row in df.iterrows():
        coords.add((row["node_i_x"], row["node_i_y"], row["node_i_z"]))
        coords.add((row["node_j_x"], row["node_j_y"], row["node_j_z"]))

    # Sort by z → x → y (ascending)
    sorted_coords = sorted(coords, key=lambda c: (c[2], c[0], c[1]))

    # Assign IDs starting from 1 (NOT 0)
    return [(i + 1, x, y, z) for i, (x, y, z) in enumerate(sorted_coords)]
