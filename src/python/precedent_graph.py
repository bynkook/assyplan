"""Precedent graph module for building DAG from structural member dependencies."""

from typing import Dict, List

import pandas as pd


def _is_null(value) -> bool:
    """Check if value is null or NaN, handling both scalars and pandas types."""
    try:
        return pd.isna(value)
    except (TypeError, ValueError):
        return False


def detect_cycles(dag: Dict[str, Dict[str, List[str]]]) -> bool:
    """Detect cycles in the directed acyclic graph using DFS.

    Uses three-color DFS algorithm:
    - WHITE (0): Node not yet visited
    - GRAY (1): Node currently being processed (in current DFS path)
    - BLACK (2): Node fully processed (all descendants visited)

    Args:
        dag: Dictionary representing the DAG with structure:
            {member_id: {"precedents": [...], "successors": [...]}}

    Returns:
        True if a cycle exists in the graph, False otherwise.
    """
    # Color states: 0 = WHITE (unvisited), 1 = GRAY (in progress), 2 = BLACK (done)
    color: Dict[str, int] = {node: 0 for node in dag}

    def dfs(node: str) -> bool:
        """Run DFS from given node to detect cycles.

        Returns True if cycle is detected, False otherwise.
        """
        color[node] = 1  # Mark as GRAY (in progress)

        # Visit all successors
        for successor in dag[node].get("successors", []):
            if successor not in dag:
                # Skip if successor doesn't exist in our DAG
                continue

            if color[successor] == 1:
                # Found a back edge - cycle detected
                return True
            if color[successor] == 0:
                # Unvisited node - recurse
                if dfs(successor):
                    return True

        color[node] = 2  # Mark as BLACK (fully processed)
        return False

    # Check all nodes (handles disconnected components)
    for node in dag:
        if color[node] == 0:
            if dfs(node):
                return True

    return False


def build_dag(df: pd.DataFrame) -> Dict[str, Dict[str, List[str]]]:
    """Build a directed acyclic graph from precedent relationships.

    Extracts 부재ID and 선행부재ID columns from the DataFrame and builds
    a bidirectional adjacency list representation of the DAG.

    Args:
        df: pandas DataFrame containing at least:
            - 부재ID: Member ID column
            - 선행부재ID: Precedent member ID column (can be empty/NaN)

    Returns:
        Dictionary representing the DAG with structure:
            {member_id: {"precedents": [list of precedent member IDs],
                         "successors": [list of successor member IDs]}}

    Raises:
        ValueError: If a cycle is detected in the precedent relationships.
    """
    # Extract required columns
    if "부재ID" not in df.columns:
        raise ValueError("Missing required column: 부재ID")
    if "선행부재ID" not in df.columns:
        raise ValueError("Missing required column: 선행부재ID")

    # Initialize DAG structure
    dag: Dict[str, Dict[str, List[str]]] = {}

    # First pass: create all nodes
    for member_id in df["부재ID"]:
        member_id_str = str(member_id)
        if member_id_str not in dag:
            dag[member_id_str] = {"precedents": [], "successors": []}

    # Second pass: build edges from precedent relationships
    for _, row in df.iterrows():
        member_id = str(row["부재ID"])
        precedent_id = row["선행부재ID"]

        # Handle empty/NaN precedent (no predecessor)
        if _is_null(precedent_id) or str(precedent_id).strip() == "":
            continue

        precedent_str = str(precedent_id).strip()

        # Skip if precedent doesn't exist in the data
        if precedent_str not in dag:
            continue

        # Add edge: precedent -> member (precedent precedes member)
        dag[precedent_str]["successors"].append(member_id)
        dag[member_id]["precedents"].append(precedent_str)

    # Check for cycles
    if detect_cycles(dag):
        raise ValueError(
            "Cycle detected in precedent relationships. The graph must be a DAG."
        )

    return dag


if __name__ == "__main__":
    # Simple test cases
    print("Running precedent graph tests...")

    # Test 1: Simple linear chain (no cycle)
    test_df1 = pd.DataFrame(
        {
            "부재ID": ["A", "B", "C"],
            "선행부재ID": ["", "A", "B"],  # A -> B -> C
        }
    )
    dag1 = build_dag(test_df1)
    print(f"Test 1 (linear chain): {dag1}")
    assert detect_cycles(dag1) == False, "Should not detect cycle in linear chain"

    # Test 2: Diamond shape (no cycle)
    test_df2 = pd.DataFrame(
        {
            "부재ID": ["A", "B", "C", "D"],
            "선행부재ID": ["", "A", "A", "B"],  # A -> B, A -> C, B -> D
        }
    )
    dag2 = build_dag(test_df2)
    print(f"Test 2 (diamond): {dag2}")
    assert detect_cycles(dag2) == False, "Should not detect cycle in diamond"

    # Test 3: Simple cycle (should raise error)
    test_df3 = pd.DataFrame(
        {
            "부재ID": ["A", "B", "C"],
            "선행부재ID": ["C", "A", "B"],  # A -> B -> C -> A (cycle!)
        }
    )
    try:
        dag3 = build_dag(test_df3)
        print("ERROR: Should have detected cycle!")
    except ValueError as e:
        print(f"Test 3 (cycle detected correctly): {e}")

    # Test 4: Multiple roots
    test_df4 = pd.DataFrame(
        {
            "부재ID": ["A", "B", "C", "D"],
            "선행부재ID": ["", "", "A", "B"],  # A -> C, B -> D
        }
    )
    dag4 = build_dag(test_df4)
    print(f"Test 4 (multiple roots): {dag4}")
    assert detect_cycles(dag4) == False

    # Test 5: NaN handling
    test_df5 = pd.DataFrame(
        {
            "부재ID": ["A", "B", "C"],
            "선행부재ID": ["", None, "A"],  # B has no precedent, A -> C
        }
    )
    dag5 = build_dag(test_df5)
    print(f"Test 5 (NaN handling): {dag5}")
    assert detect_cycles(dag5) == False

    print("\nAll tests passed!")
