"""Tests for sequence_table module."""

import json
import os
import tempfile

import pandas as pd
import pytest

from src.python.precedent_graph import build_dag
from src.python.sequence_table import (
    create_sequence_table,
    save_sequence_table,
    topological_sort,
)
from src.python.workfront import get_workfront_members


class TestTopologicalSort:
    """Tests for topological_sort function."""

    def test_linear_chain(self):
        """Test topological sort with simple linear chain."""
        dag = {
            "A": {"precedents": [], "successors": ["B"]},
            "B": {"precedents": ["A"], "successors": ["C"]},
            "C": {"precedents": ["B"], "successors": []},
        }
        result = topological_sort(dag)

        assert result == ["A", "B", "C"]

    def test_diamond_shape(self):
        """Test topological sort with diamond dependency."""
        dag = {
            "A": {"precedents": [], "successors": ["B", "C"]},
            "B": {"precedents": ["A"], "successors": ["D"]},
            "C": {"precedents": ["A"], "successors": ["D"]},
            "D": {"precedents": ["B", "C"], "successors": []},
        }
        result = topological_sort(dag)

        assert result[0] == "A"
        assert result[-1] == "D"
        # B and C should come before D
        assert result.index("B") < result.index("D")
        assert result.index("C") < result.index("D")

    def test_multiple_roots(self):
        """Test topological sort with multiple root nodes."""
        dag = {
            "A": {"precedents": [], "successors": ["C"]},
            "B": {"precedents": [], "successors": ["D"]},
            "C": {"precedents": ["A"], "successors": []},
            "D": {"precedents": ["B"], "successors": []},
        }
        result = topological_sort(dag)

        # A and B should be first (in sorted order: A, B)
        assert result[0] == "A"
        assert result[1] == "B"

    def test_empty_dag(self):
        """Test topological sort with empty DAG."""
        result = topological_sort({})
        assert result == []

    def test_single_node(self):
        """Test topological sort with single node."""
        dag = {
            "A": {"precedents": [], "successors": []},
        }
        result = topological_sort(dag)
        assert result == ["A"]

    def test_cycle_detection(self):
        """Test that cycle raises ValueError."""
        dag = {
            "A": {"precedents": ["C"], "successors": ["B"]},
            "B": {"precedents": ["A"], "successors": ["C"]},
            "C": {"precedents": ["B"], "successors": ["A"]},
        }
        with pytest.raises(ValueError, match="Cycle detected"):
            topological_sort(dag)

    def test_complex_dag(self):
        """Test topological sort with more complex DAG."""
        #    A -> B -> D -> E
        #    |    |
        #    v    v
        #    C ----> F
        dag = {
            "A": {"precedents": [], "successors": ["B", "C"]},
            "B": {"precedents": ["A"], "successors": ["D"]},
            "C": {"precedents": ["A"], "successors": ["F"]},
            "D": {"precedents": ["B"], "successors": ["E"]},
            "E": {"precedents": ["D"], "successors": []},
            "F": {"precedents": ["C"], "successors": []},
        }
        result = topological_sort(dag)

        # A must be first
        assert result[0] == "A"
        # E and F are terminal nodes (no successors), either can be last
        assert result[-1] in ["E", "F"]
        # B must come after A
        assert result.index("A") < result.index("B")
        # D must come after B
        assert result.index("B") < result.index("D")
        # E must come after D
        assert result.index("D") < result.index("E")


class TestCreateSequenceTable:
    """Tests for create_sequence_table function."""

    def test_simple_two_workfronts(self):
        """Test creating sequence table with two workfronts."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B", "C", "D"],
                "선행부재ID": ["", "", "A", "B"],
            }
        )
        dag = build_dag(df)
        workfronts = get_workfront_members(df, dag)

        sequence = create_sequence_table(dag, workfronts)

        # Workfront 1: A -> C
        # Workfront 2: B -> D
        # Sequence should be: (1, A), (1, C), (2, B), (2, D)
        assert sequence[0] == (1, "A")
        assert sequence[1] == (1, "C")
        assert sequence[2] == (2, "B")
        assert sequence[3] == (2, "D")

    def test_single_workfront_chain(self):
        """Test sequence table with single workfront linear chain."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B", "C"],
                "선행부재ID": ["", "A", "B"],
            }
        )
        dag = build_dag(df)
        workfronts = get_workfront_members(df, dag)

        sequence = create_sequence_table(dag, workfronts)

        assert sequence == [(1, "A"), (1, "B"), (1, "C")]

    def test_workfronts_in_topo_order_within(self):
        """Test that members within workfront are in topological order."""
        # Diamond shape: A -> B, A -> C, B -> D, C -> D
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B", "C", "D"],
                "선행부재ID": ["", "A", "A", "B"],
            }
        )
        dag = build_dag(df)
        workfronts = get_workfront_members(df, dag)

        # All members belong to workfront 1
        assert len(workfronts) == 1

        sequence = create_sequence_table(dag, workfronts)

        # A must come first, D must come last
        assert sequence[0] == (1, "A")
        assert sequence[-1] == (1, "D")

    def test_empty_dag_raises_error(self):
        """Test that empty DAG raises ValueError."""
        workfronts = {1: ["A"]}

        with pytest.raises(ValueError, match="DAG cannot be empty"):
            create_sequence_table({}, workfronts)

    def test_empty_workfronts_raises_error(self):
        """Test that empty workfronts raises ValueError."""
        dag = {"A": {"precedents": [], "successors": []}}

        with pytest.raises(ValueError, match="Workfronts cannot be empty"):
            create_sequence_table(dag, {})

    def test_multiple_workfronts_complex(self):
        """Test sequence table with multiple complex workfronts."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B", "C", "D", "E", "F"],
                "선행부재ID": ["", "", "A", "B", "C", "B"],
            }
        )
        dag = build_dag(df)
        workfronts = get_workfront_members(df, dag)

        sequence = create_sequence_table(dag, workfronts)

        # Workfront 1: A -> C -> E
        # Workfront 2: B -> D, B -> F
        # Within workfront 1, A must come before C, C before E
        wf1_members = [(wf, m) for wf, m in sequence if wf == 1]
        assert wf1_members[0] == (1, "A")
        assert wf1_members[1][1] == "C"  # C before E
        assert wf1_members[-1][1] == "E"


class TestSaveSequenceTable:
    """Tests for save_sequence_table function."""

    def test_save_csv(self):
        """Test saving sequence table to CSV."""
        sequence = [(1, "A"), (1, "B"), (2, "C")]

        with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
            filepath = f.name

        try:
            save_sequence_table(sequence, filepath, format="csv")

            # Read back and verify
            df = pd.read_csv(filepath)
            assert len(df) == 3
            assert df.columns.tolist() == ["workfront_id", "member_id"]
            assert df["workfront_id"].tolist() == [1, 1, 2]
            assert df["member_id"].tolist() == ["A", "B", "C"]
        finally:
            os.unlink(filepath)

    def test_save_json(self):
        """Test saving sequence table to JSON."""
        sequence = [(1, "A"), (1, "B"), (2, "C")]

        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            filepath = f.name

        try:
            save_sequence_table(sequence, filepath, format="json")

            # Read back and verify
            with open(filepath, "r", encoding="utf-8") as f:
                data = json.load(f)

            assert len(data) == 3
            assert data[0] == {"workfront_id": 1, "member_id": "A"}
            assert data[1] == {"workfront_id": 1, "member_id": "B"}
            assert data[2] == {"workfront_id": 2, "member_id": "C"}
        finally:
            os.unlink(filepath)

    def test_empty_sequence_raises_error(self):
        """Test that empty sequence raises ValueError."""
        with pytest.raises(ValueError, match="Sequence cannot be empty"):
            save_sequence_table([], "test.csv")

    def test_unsupported_format_raises_error(self):
        """Test that unsupported format raises ValueError."""
        sequence = [(1, "A")]

        with pytest.raises(ValueError, match="Unsupported format"):
            save_sequence_table(sequence, "test.txt", format="txt")

    def test_csv_format_case_insensitive(self):
        """Test that CSV format is case insensitive."""
        sequence = [(1, "A"), (2, "B")]

        with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
            filepath = f.name

        try:
            save_sequence_table(sequence, filepath, format="CSV")
            df = pd.read_csv(filepath)
            assert len(df) == 2
        finally:
            os.unlink(filepath)

    def test_json_format_case_insensitive(self):
        """Test that JSON format is case insensitive."""
        sequence = [(1, "A"), (2, "B")]

        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            filepath = f.name

        try:
            save_sequence_table(sequence, filepath, format="Json")
            with open(filepath, "r", encoding="utf-8") as f:
                data = json.load(f)
            assert len(data) == 2
        finally:
            os.unlink(filepath)


class TestIntegration:
    """Integration tests combining all functions."""

    def test_full_workflow(self):
        """Test full workflow from DataFrame to saved file."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B", "C", "D", "E", "F"],
                "선행부재ID": ["", "", "A", "B", "C", "B"],
            }
        )
        dag = build_dag(df)
        workfronts = get_workfront_members(df, dag)

        # Get topological order
        topo_order = topological_sort(dag)
        assert len(topo_order) == 6

        # Create sequence table
        sequence = create_sequence_table(dag, workfronts)
        assert len(sequence) == 6

        # Save to CSV
        with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
            csv_path = f.name

        try:
            save_sequence_table(sequence, csv_path, format="csv")
            df_result = pd.read_csv(csv_path)
            assert len(df_result) == 6
            assert "workfront_id" in df_result.columns
            assert "member_id" in df_result.columns
        finally:
            os.unlink(csv_path)

        # Save to JSON
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            json_path = f.name

        try:
            save_sequence_table(sequence, json_path, format="json")
            with open(json_path, "r", encoding="utf-8") as f:
                data = json.load(f)
            assert len(data) == 6
            assert all("workfront_id" in item for item in data)
            assert all("member_id" in item for item in data)
        finally:
            os.unlink(json_path)
