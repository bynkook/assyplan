"""Workfront identification module for structural analysis scheduling.

This module identifies workfront start points from structural members that have
no preceding members (precedent), and provides utilities to get all members
belonging to each workfront.
"""

from typing import Dict, List, Tuple

import pandas as pd


def _is_null(value) -> bool:
    """Check if value is null or NaN, handling both scalars and pandas types."""
    try:
        return pd.isna(value)
    except (TypeError, ValueError):
        return False


def identify_workfronts(df: pd.DataFrame) -> List[Tuple[int, str]]:
    """Identify workfront start points from members with no precedent.

    Workfronts start at members where 선행부재ID (precedent member ID) is empty/NaN.
    Each such member becomes the start of a new workfront.

    Args:
        df: pandas DataFrame containing at least:
            - 부재ID: Member ID column
            - 선행부재ID: Precedent member ID column (can be empty/NaN)

    Returns:
        List of (workfront_id, member_id) tuples sorted by member_id.
        workfront_id starts from 1 (NOT 0).

    Raises:
        ValueError: If required columns are missing.
        ValueError: If all members have precedents (no workfront can start).
    """
    # Validate required columns
    if "부재ID" not in df.columns:
        raise ValueError("Missing required column: 부재ID")
    if "선행부재ID" not in df.columns:
        raise ValueError("Missing required column: 선행부재ID")

    # Check for empty DataFrame
    if df.empty:
        raise ValueError("DataFrame is empty")

    # Find members with no precedent (empty/NaN 선행부재ID)
    workfront_starts: List[str] = []
    for _, row in df.iterrows():
        member_id = str(row["부재ID"])
        precedent_id = row["선행부재ID"]

        # Check if precedent is empty/null
        if _is_null(precedent_id) or str(precedent_id).strip() == "":
            workfront_starts.append(member_id)

    # Check edge case: no workfront starts found
    if not workfront_starts:
        raise ValueError(
            "No workfront start points found. All members have precedents."
        )

    # Sort by member_id and assign workfront_ids starting from 1
    workfront_starts_sorted = sorted(workfront_starts)
    workfronts: List[Tuple[int, str]] = [
        (workfront_id, member_id)
        for workfront_id, member_id in enumerate(workfront_starts_sorted, start=1)
    ]

    return workfronts


def get_workfront_members(
    df: pd.DataFrame, dag: Dict[str, Dict[str, List[str]]]
) -> Dict[int, List[str]]:
    """Get all members belonging to each workfront by traversing the DAG.

    Starting from each workfront start point (member with no precedent),
    traverses the DAG forward through successors to find all members
    belonging to that workfront.

    Args:
        df: pandas DataFrame containing at least:
            - 부재ID: Member ID column
            - 선행부재ID: Precedent member ID column
        dag: Dictionary representing the DAG with structure:
            {member_id: {"precedents": [...], "successors": [...]}}

    Returns:
        Dictionary mapping workfront_id (starting from 1) to list of
        member_ids belonging to that workfront.

    Raises:
        ValueError: If required columns are missing.
        ValueError: If DataFrame is empty.
    """
    # Validate required columns
    if "부재ID" not in df.columns:
        raise ValueError("Missing required column: 부재ID")
    if "선행부재ID" not in df.columns:
        raise ValueError("Missing required column: 선행부재ID")

    # Check for empty DataFrame
    if df.empty:
        raise ValueError("DataFrame is empty")

    # First identify workfront start points
    workfront_starts = identify_workfronts(df)

    # For each workfront start, traverse forward through successors
    result: Dict[int, List[str]] = {}

    for workfront_id, start_member in workfront_starts:
        # Use BFS/DFS to find all successors
        members_in_workfront: List[str] = [start_member]
        visited: set[str] = {start_member}
        queue: List[str] = [start_member]

        while queue:
            current = queue.pop(0)
            # Get successors from DAG if available
            if current in dag:
                for successor in dag[current].get("successors", []):
                    if successor not in visited:
                        visited.add(successor)
                        members_in_workfront.append(successor)
                        queue.append(successor)

        # Sort members by their ID for consistent output
        result[workfront_id] = sorted(members_in_workfront)

    return result


if __name__ == "__main__":
    # Simple test cases
    print("Running workfront identification tests...")

    # Test 1: Simple case with 2 workfronts
    test_df1 = pd.DataFrame(
        {
            "부재ID": ["A", "B", "C", "D"],
            "선행부재ID": ["", "", "A", "B"],  # A, B are workfront starts
        }
    )
    workfronts1 = identify_workfronts(test_df1)
    print(f"Test 1 - Workfront starts: {workfronts1}")
    # Expected: [(1, 'A'), (2, 'B')]

    # Test 2: Single workfront with chain
    test_df2 = pd.DataFrame(
        {
            "부재ID": ["A", "B", "C"],
            "선행부재ID": ["", "A", "B"],  # Only A is workfront start
        }
    )
    workfronts2 = identify_workfronts(test_df2)
    print(f"Test 2 - Workfront starts: {workfronts2}")
    # Expected: [(1, 'A')]

    # Test 3: Test get_workfront_members
    from src.python.precedent_graph import build_dag

    dag3 = build_dag(test_df1)
    members3 = get_workfront_members(test_df1, dag3)
    print(f"Test 3 - Workfront members: {members3}")
    # Expected: {1: ['A', 'C'], 2: ['B', 'D']}

    # Test 4: NaN handling
    test_df4 = pd.DataFrame(
        {
            "부재ID": ["A", "B", "C"],
            "선행부재ID": [None, "A", None],  # A and C are workfront starts
        }
    )
    workfronts4 = identify_workfronts(test_df4)
    print(f"Test 4 - Workfront starts (NaN): {workfronts4}")
    # Expected: [(1, 'A'), (2, 'C')]

    # Test 5: Empty DataFrame
    test_df5 = pd.DataFrame(
        {
            "부재ID": [],
            "선행부재ID": [],
        }
    )
    try:
        identify_workfronts(test_df5)
        print("ERROR: Should have raised ValueError for empty DataFrame")
    except ValueError as e:
        print(f"Test 5 - Empty DataFrame error (expected): {e}")

    # Test 6: All members have precedents
    test_df6 = pd.DataFrame(
        {
            "부재ID": ["A", "B"],
            "선행부재ID": ["B", "A"],  # Cycle - but error should be from no workfronts
        }
    )
    try:
        identify_workfronts(test_df6)
        print("ERROR: Should have raised ValueError for no workfronts")
    except ValueError as e:
        print(f"Test 6 - No workfronts error (expected): {e}")

    print("\nAll tests completed!")
