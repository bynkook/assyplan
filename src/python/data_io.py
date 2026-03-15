"""Data I/O module for serialization and deserialization of structural data.

This module provides functions to save and load:
- Node tables
- Element tables
- Step tables
- Complete project data (all tables bundled)

Supported formats: CSV, JSON
"""

import json
from pathlib import Path
from typing import Any, Dict, List, Tuple, Union

import pandas as pd


# Type aliases for clarity
NodeTable = List[Tuple[int, float, float, float]]  # (node_id, x, y, z)
ElementTable = List[
    Tuple[int, int, int, str]
]  # (element_id, node_i_id, node_j_id, member_type)
StepTable = List[Tuple[int, int, str]]  # (workfront_id, step, member_id)


def save_node_table(
    nodes: NodeTable,
    filepath: Union[str, Path],
    format: str = "csv",
) -> None:
    """Save node table to file.

    Args:
        nodes: List of (node_id, x, y, z) tuples.
        filepath: Output file path.
        format: Output format - "csv" or "json". Defaults to "csv".

    Raises:
        ValueError: If format is not "csv" or "json".
        ValueError: If nodes is empty.

    Example:
        >>> nodes = [(1, 0.0, 0.0, 0.0), (2, 1.0, 0.0, 0.0)]
        >>> save_node_table(nodes, "nodes.csv")
    """
    if not nodes:
        raise ValueError("Node table cannot be empty")

    filepath = Path(filepath)

    if format.lower() == "csv":
        df = pd.DataFrame(nodes, columns=["node_id", "x", "y", "z"])
        df.to_csv(filepath, index=False)
    elif format.lower() == "json":
        data = [{"node_id": nid, "x": x, "y": y, "z": z} for nid, x, y, z in nodes]
        with open(filepath, "w", encoding="utf-8") as f:
            json.dump(data, f, ensure_ascii=False, indent=2)
    else:
        raise ValueError(f"Unsupported format: {format}. Use 'csv' or 'json'.")


def load_node_table(
    filepath: Union[str, Path],
    format: str = "csv",
) -> NodeTable:
    """Load node table from file.

    Args:
        filepath: Input file path.
        format: Input format - "csv" or "json". Defaults to "csv".

    Returns:
        List of (node_id, x, y, z) tuples.

    Raises:
        ValueError: If format is not "csv" or "json".
        FileNotFoundError: If file does not exist.

    Example:
        >>> nodes = load_node_table("nodes.csv")
        >>> print(nodes[0])  # (1, 0.0, 0.0, 0.0)
    """
    filepath = Path(filepath)

    if not filepath.exists():
        raise FileNotFoundError(f"File not found: {filepath}")

    if format.lower() == "csv":
        df = pd.read_csv(filepath)
        return [
            (int(row["node_id"]), float(row["x"]), float(row["y"]), float(row["z"]))
            for _, row in df.iterrows()
        ]
    elif format.lower() == "json":
        with open(filepath, "r", encoding="utf-8") as f:
            data = json.load(f)
        return [
            (int(item["node_id"]), float(item["x"]), float(item["y"]), float(item["z"]))
            for item in data
        ]
    else:
        raise ValueError(f"Unsupported format: {format}. Use 'csv' or 'json'.")


def save_element_table(
    elements: ElementTable,
    filepath: Union[str, Path],
    format: str = "csv",
) -> None:
    """Save element table to file.

    Args:
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.
        filepath: Output file path.
        format: Output format - "csv" or "json". Defaults to "csv".

    Raises:
        ValueError: If format is not "csv" or "json".
        ValueError: If elements is empty.

    Example:
        >>> elements = [(1, 1, 2, "Column"), (2, 2, 3, "Girder")]
        >>> save_element_table(elements, "elements.csv")
    """
    if not elements:
        raise ValueError("Element table cannot be empty")

    filepath = Path(filepath)

    if format.lower() == "csv":
        df = pd.DataFrame(
            elements,
            columns=["element_id", "node_i_id", "node_j_id", "member_type"],
        )
        df.to_csv(filepath, index=False)
    elif format.lower() == "json":
        data = [
            {
                "element_id": eid,
                "node_i_id": ni,
                "node_j_id": nj,
                "member_type": mtype,
            }
            for eid, ni, nj, mtype in elements
        ]
        with open(filepath, "w", encoding="utf-8") as f:
            json.dump(data, f, ensure_ascii=False, indent=2)
    else:
        raise ValueError(f"Unsupported format: {format}. Use 'csv' or 'json'.")


def load_element_table(
    filepath: Union[str, Path],
    format: str = "csv",
) -> ElementTable:
    """Load element table from file.

    Args:
        filepath: Input file path.
        format: Input format - "csv" or "json". Defaults to "csv".

    Returns:
        List of (element_id, node_i_id, node_j_id, member_type) tuples.

    Raises:
        ValueError: If format is not "csv" or "json".
        FileNotFoundError: If file does not exist.

    Example:
        >>> elements = load_element_table("elements.csv")
        >>> print(elements[0])  # (1, 1, 2, 'Column')
    """
    filepath = Path(filepath)

    if not filepath.exists():
        raise FileNotFoundError(f"File not found: {filepath}")

    if format.lower() == "csv":
        df = pd.read_csv(filepath)
        return [
            (
                int(row["element_id"]),
                int(row["node_i_id"]),
                int(row["node_j_id"]),
                str(row["member_type"]),
            )
            for _, row in df.iterrows()
        ]
    elif format.lower() == "json":
        with open(filepath, "r", encoding="utf-8") as f:
            data = json.load(f)
        return [
            (
                int(item["element_id"]),
                int(item["node_i_id"]),
                int(item["node_j_id"]),
                str(item["member_type"]),
            )
            for item in data
        ]
    else:
        raise ValueError(f"Unsupported format: {format}. Use 'csv' or 'json'.")


def save_step_table(
    step_table: StepTable,
    filepath: Union[str, Path],
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

    Example:
        >>> steps = [(1, 1, "A"), (1, 1, "B"), (1, 2, "C")]
        >>> save_step_table(steps, "steps.csv")
    """
    if not step_table:
        raise ValueError("Step table cannot be empty")

    filepath = Path(filepath)

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


def load_step_table(
    filepath: Union[str, Path],
    format: str = "csv",
) -> StepTable:
    """Load workfront step table from file.

    Args:
        filepath: Input file path.
        format: Input format - "csv" or "json". Defaults to "csv".

    Returns:
        List of (workfront_id, step, member_id) tuples.

    Raises:
        ValueError: If format is not "csv" or "json".
        FileNotFoundError: If file does not exist.

    Example:
        >>> steps = load_step_table("steps.csv")
        >>> print(steps[0])  # (1, 1, 'A')
    """
    filepath = Path(filepath)

    if not filepath.exists():
        raise FileNotFoundError(f"File not found: {filepath}")

    if format.lower() == "csv":
        df = pd.read_csv(filepath)
        return [
            (int(row["workfront_id"]), int(row["step"]), str(row["member_id"]))
            for _, row in df.iterrows()
        ]
    elif format.lower() == "json":
        with open(filepath, "r", encoding="utf-8") as f:
            data = json.load(f)
        return [
            (int(item["workfront_id"]), int(item["step"]), str(item["member_id"]))
            for item in data
        ]
    else:
        raise ValueError(f"Unsupported format: {format}. Use 'csv' or 'json'.")


def save_project_data(
    nodes: NodeTable,
    elements: ElementTable,
    step_table: StepTable,
    filepath: Union[str, Path],
) -> None:
    """Save complete project data to a single JSON file.

    Bundles node table, element table, and step table into one file
    for easy project persistence and sharing.

    Args:
        nodes: List of (node_id, x, y, z) tuples.
        elements: List of (element_id, node_i_id, node_j_id, member_type) tuples.
        step_table: List of (workfront_id, step, member_id) tuples.
        filepath: Output file path (JSON format).

    Raises:
        ValueError: If any table is empty.

    Example:
        >>> save_project_data(nodes, elements, steps, "project.json")
    """
    if not nodes:
        raise ValueError("Node table cannot be empty")
    if not elements:
        raise ValueError("Element table cannot be empty")
    if not step_table:
        raise ValueError("Step table cannot be empty")

    filepath = Path(filepath)

    data: Dict[str, Any] = {
        "version": "1.0",
        "nodes": [{"node_id": nid, "x": x, "y": y, "z": z} for nid, x, y, z in nodes],
        "elements": [
            {
                "element_id": eid,
                "node_i_id": ni,
                "node_j_id": nj,
                "member_type": mtype,
            }
            for eid, ni, nj, mtype in elements
        ],
        "step_table": [
            {"workfront_id": wf_id, "step": step, "member_id": member_id}
            for wf_id, step, member_id in step_table
        ],
    }

    with open(filepath, "w", encoding="utf-8") as f:
        json.dump(data, f, ensure_ascii=False, indent=2)


def load_project_data(
    filepath: Union[str, Path],
) -> Tuple[NodeTable, ElementTable, StepTable]:
    """Load complete project data from a JSON file.

    Args:
        filepath: Input file path (JSON format).

    Returns:
        Tuple of (nodes, elements, step_table).

    Raises:
        FileNotFoundError: If file does not exist.
        KeyError: If required keys are missing from the file.

    Example:
        >>> nodes, elements, steps = load_project_data("project.json")
    """
    filepath = Path(filepath)

    if not filepath.exists():
        raise FileNotFoundError(f"File not found: {filepath}")

    with open(filepath, "r", encoding="utf-8") as f:
        data = json.load(f)

    # Parse nodes
    nodes: NodeTable = [
        (int(item["node_id"]), float(item["x"]), float(item["y"]), float(item["z"]))
        for item in data["nodes"]
    ]

    # Parse elements
    elements: ElementTable = [
        (
            int(item["element_id"]),
            int(item["node_i_id"]),
            int(item["node_j_id"]),
            str(item["member_type"]),
        )
        for item in data["elements"]
    ]

    # Parse step table
    step_table: StepTable = [
        (int(item["workfront_id"]), int(item["step"]), str(item["member_id"]))
        for item in data["step_table"]
    ]

    return nodes, elements, step_table


if __name__ == "__main__":
    import tempfile

    print("Running data_io tests...")

    # Test data
    test_nodes: NodeTable = [
        (1, 0.0, 0.0, 0.0),
        (2, 1.0, 0.0, 0.0),
        (3, 0.0, 0.0, 3.0),
    ]
    test_elements: ElementTable = [
        (1, 1, 3, "Column"),
        (2, 2, 3, "Girder"),
    ]
    test_steps: StepTable = [
        (1, 1, "A"),
        (1, 1, "B"),
        (1, 2, "C"),
    ]

    with tempfile.TemporaryDirectory() as tmpdir:
        tmpdir_path = Path(tmpdir)

        # Test node table CSV
        node_csv = tmpdir_path / "nodes.csv"
        save_node_table(test_nodes, node_csv, "csv")
        loaded_nodes = load_node_table(node_csv, "csv")
        assert loaded_nodes == test_nodes, f"Node CSV mismatch: {loaded_nodes}"
        print("  Node table CSV: OK")

        # Test node table JSON
        node_json = tmpdir_path / "nodes.json"
        save_node_table(test_nodes, node_json, "json")
        loaded_nodes = load_node_table(node_json, "json")
        assert loaded_nodes == test_nodes, f"Node JSON mismatch: {loaded_nodes}"
        print("  Node table JSON: OK")

        # Test element table CSV
        elem_csv = tmpdir_path / "elements.csv"
        save_element_table(test_elements, elem_csv, "csv")
        loaded_elements = load_element_table(elem_csv, "csv")
        assert loaded_elements == test_elements, (
            f"Element CSV mismatch: {loaded_elements}"
        )
        print("  Element table CSV: OK")

        # Test element table JSON
        elem_json = tmpdir_path / "elements.json"
        save_element_table(test_elements, elem_json, "json")
        loaded_elements = load_element_table(elem_json, "json")
        assert loaded_elements == test_elements, (
            f"Element JSON mismatch: {loaded_elements}"
        )
        print("  Element table JSON: OK")

        # Test step table CSV
        step_csv = tmpdir_path / "steps.csv"
        save_step_table(test_steps, step_csv, "csv")
        loaded_steps = load_step_table(step_csv, "csv")
        assert loaded_steps == test_steps, f"Step CSV mismatch: {loaded_steps}"
        print("  Step table CSV: OK")

        # Test step table JSON
        step_json = tmpdir_path / "steps.json"
        save_step_table(test_steps, step_json, "json")
        loaded_steps = load_step_table(step_json, "json")
        assert loaded_steps == test_steps, f"Step JSON mismatch: {loaded_steps}"
        print("  Step table JSON: OK")

        # Test project data
        project_json = tmpdir_path / "project.json"
        save_project_data(test_nodes, test_elements, test_steps, project_json)
        loaded_n, loaded_e, loaded_s = load_project_data(project_json)
        assert loaded_n == test_nodes, f"Project nodes mismatch"
        assert loaded_e == test_elements, f"Project elements mismatch"
        assert loaded_s == test_steps, f"Project steps mismatch"
        print("  Project data JSON: OK")

        # Test error cases
        try:
            save_node_table([], node_csv)
            assert False, "Should raise ValueError for empty nodes"
        except ValueError:
            print("  Empty nodes validation: OK")

        try:
            load_node_table(tmpdir_path / "nonexistent.csv")
            assert False, "Should raise FileNotFoundError"
        except FileNotFoundError:
            print("  FileNotFoundError: OK")

        try:
            save_node_table(test_nodes, node_csv, "xml")
            assert False, "Should raise ValueError for unsupported format"
        except ValueError:
            print("  Unsupported format validation: OK")

    print("\nAll data_io tests passed!")
