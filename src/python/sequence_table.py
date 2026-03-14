"""Construction Sequence Table module for generating ordered member sequences.

This module provides functions to:
- Perform topological sort on a DAG representing member precedents
- Combine topological order with workfront assignments
- Save the resulting sequence table to CSV or JSON
"""

import json
from typing import Dict, List, Tuple

import pandas as pd


def topological_sort(dag: Dict[str, Dict[str, List[str]]]) -> List[str]:
    """Perform topological sort on a DAG using Kahn's algorithm (BFS-based).

    This function returns a valid construction order by processing nodes
    in topological order. When multiple nodes have no remaining precedents,
    they are processed in sorted order by member_id.

    Args:
        dag: Dictionary representing the DAG with structure:
            {member_id: {"precedents": [list of precedent member IDs],
                         "successors": [list of successor member IDs]}}

    Returns:
        List of member_ids in topological order (valid construction sequence).

    Raises:
        ValueError: If the graph contains a cycle (not a valid DAG).
    """
    if not dag:
        return []

    # Create a copy of precedents count for each node
    # in_degree = number of precedents (incoming edges)
    in_degree: Dict[str, int] = {}
    for node, edges in dag.items():
        in_degree[node] = len(edges.get("precedents", []))

    # Start with nodes that have no precedents (in_degree == 0)
    # Use a sorted queue for deterministic ordering
    queue: List[str] = sorted(
        [node for node, degree in in_degree.items() if degree == 0]
    )

    result: List[str] = []

    while queue:
        # Process node with no remaining precedents
        node = queue.pop(0)
        result.append(node)

        # Reduce in_degree for all successors
        for successor in dag[node].get("successors", []):
            if successor in in_degree:
                in_degree[successor] -= 1
                if in_degree[successor] == 0:
                    # Add to queue in sorted order
                    queue.append(successor)
                    queue.sort()

    # Check if all nodes were processed (no cycles)
    if len(result) != len(dag):
        raise ValueError(
            "Cycle detected in the graph. Topological sort is only valid for DAGs."
        )

    return result


def create_sequence_table(
    dag: Dict[str, Dict[str, List[str]]],
    workfronts: Dict[int, List[str]],
) -> List[Tuple[int, str]]:
    """Create construction sequence table combining topological order with workfronts.

    Each workfront's members appear in topological order within that workfront.
    Workfronts are processed in order (workfront_id ascending).

    Args:
        dag: Dictionary representing the DAG with structure:
            {member_id: {"precedents": [list of precedent member IDs],
                         "successors": [list of successor member IDs]}}
        workfronts: Dictionary mapping workfront_id (starting from 1) to
            list of member_ids belonging to that workfront.
            This is the output of get_workfront_members().

    Returns:
        List of (workfront_id, member_id) tuples in construction sequence order.
        Each workfront's members appear in topological order.

    Raises:
        ValueError: If dag is empty or workfronts is empty.
    """
    if not dag:
        raise ValueError("DAG cannot be empty")
    if not workfronts:
        raise ValueError("Workfronts cannot be empty")

    # Get topological order for all members
    topo_order = topological_sort(dag)
    topo_index = {member: idx for idx, member in enumerate(topo_order)}

    # Build sequence: for each workfront, order members by topological position
    result: List[Tuple[int, str]] = []

    # Process workfronts in ascending order
    for workfront_id in sorted(workfronts.keys()):
        members = workfronts[workfront_id]
        # Sort members by their position in topological order
        sorted_members = sorted(members, key=lambda m: topo_index.get(m, float("inf")))

        for member_id in sorted_members:
            result.append((workfront_id, member_id))

    return result


def save_sequence_table(
    sequence: List[Tuple[int, str]],
    filepath: str,
    format: str = "csv",
) -> None:
    """Save construction sequence table to file.

    Args:
        sequence: List of (workfront_id, member_id) tuples.
        filepath: Output file path.
        format: Output format - "csv" or "json". Defaults to "csv".

    Raises:
        ValueError: If format is not "csv" or "json".
        ValueError: If sequence is empty.
    """
    if not sequence:
        raise ValueError("Sequence cannot be empty")

    if format.lower() == "csv":
        df = pd.DataFrame(sequence, columns=["workfront_id", "member_id"])
        df.to_csv(filepath, index=False)
    elif format.lower() == "json":
        data = [
            {"workfront_id": wf_id, "member_id": member_id}
            for wf_id, member_id in sequence
        ]
        with open(filepath, "w", encoding="utf-8") as f:
            json.dump(data, f, ensure_ascii=False, indent=2)
    else:
        raise ValueError(f"Unsupported format: {format}. Use 'csv' or 'json'.")


if __name__ == "__main__":
    # Simple test cases
    print("Running sequence table tests...")

    # Test 1: Simple linear chain
    dag1 = {
        "A": {"precedents": [], "successors": ["B"]},
        "B": {"precedents": ["A"], "successors": ["C"]},
        "C": {"precedents": ["B"], "successors": []},
    }
    topo1 = topological_sort(dag1)
    print(f"Test 1 (linear chain): {topo1}")
    assert topo1 == ["A", "B", "C"], f"Expected ['A', 'B', 'C'], got {topo1}"

    # Test 2: Diamond shape
    dag2 = {
        "A": {"precedents": [], "successors": ["B", "C"]},
        "B": {"precedents": ["A"], "successors": ["D"]},
        "C": {"precedents": ["A"], "successors": ["D"]},
        "D": {"precedents": ["B", "C"], "successors": []},
    }
    topo2 = topological_sort(dag2)
    print(f"Test 2 (diamond): {topo2}")
    assert topo2[0] == "A", f"Expected A first, got {topo2[0]}"
    assert topo2[-1] == "D", f"Expected D last, got {topo2[-1]}"

    # Test 3: Multiple roots
    dag3 = {
        "A": {"precedents": [], "successors": ["C"]},
        "B": {"precedents": [], "successors": ["D"]},
        "C": {"precedents": ["A"], "successors": []},
        "D": {"precedents": ["B"], "successors": []},
    }
    topo3 = topological_sort(dag3)
    print(f"Test 3 (multiple roots): {topo3}")
    assert topo3[0] in ["A", "B"], f"Expected A or B first, got {topo3[0]}"

    # Test 4: create_sequence_table with workfronts
    workfronts4 = {
        1: ["A", "C"],
        2: ["B", "D"],
    }
    seq4 = create_sequence_table(dag3, workfronts4)
    print(f"Test 4 (sequence table): {seq4}")
    # A and B are roots, C depends on A, D depends on B
    # Workfront 1: A, C (in topo order)
    # Workfront 2: B, D (in topo order)
    assert seq4[0] == (1, "A"), f"Expected (1, 'A'), got {seq4[0]}"

    print("\nAll tests passed!")
