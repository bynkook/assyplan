"""Tests for precedent_graph module."""

import pandas as pd
import pytest

from src.python.precedent_graph import build_dag, detect_cycles


class TestDetectCycles:
    """Tests for detect_cycles function."""

    def test_no_cycle_linear_chain(self):
        """Test that linear chain has no cycle."""
        dag = {
            "A": {"precedents": [], "successors": ["B"]},
            "B": {"precedents": ["A"], "successors": ["C"]},
            "C": {"precedents": ["B"], "successors": []},
        }
        assert detect_cycles(dag) is False

    def test_no_cycle_diamond(self):
        """Test that diamond structure has no cycle."""
        dag = {
            "A": {"precedents": [], "successors": ["B", "C"]},
            "B": {"precedents": ["A"], "successors": ["D"]},
            "C": {"precedents": ["A"], "successors": ["D"]},
            "D": {"precedents": ["B", "C"], "successors": []},
        }
        assert detect_cycles(dag) is False

    def test_no_cycle_multiple_roots(self):
        """Test that multiple root nodes have no cycle."""
        dag = {
            "A": {"precedents": [], "successors": ["C"]},
            "B": {"precedents": [], "successors": ["D"]},
            "C": {"precedents": ["A"], "successors": []},
            "D": {"precedents": ["B"], "successors": []},
        }
        assert detect_cycles(dag) is False

    def test_simple_cycle(self):
        """Test that simple cycle is detected."""
        dag = {
            "A": {"precedents": ["C"], "successors": ["B"]},
            "B": {"precedents": ["A"], "successors": ["C"]},
            "C": {"precedents": ["B"], "successors": ["A"]},
        }
        assert detect_cycles(dag) is True

    def test_cycle_with_multiple_nodes(self):
        """Test cycle detection with more complex graph."""
        dag = {
            "A": {"precedents": [], "successors": ["B"]},
            "B": {"precedents": ["A"], "successors": ["C"]},
            "C": {"precedents": ["B"], "successors": ["D"]},
            "D": {"precedents": ["C"], "successors": ["A"]},  # back edge to A
        }
        assert detect_cycles(dag) is True

    def test_self_loop(self):
        """Test that self-loop is detected as cycle."""
        dag = {
            "A": {"precedents": ["A"], "successors": ["A"]},
        }
        assert detect_cycles(dag) is True

    def test_empty_graph(self):
        """Test empty graph has no cycle."""
        dag = {}
        assert detect_cycles(dag) is False


class TestBuildDag:
    """Tests for build_dag function."""

    def test_build_dag_linear_chain(self):
        """Test DAG building with linear chain."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B", "C"],
                "선행부재ID": ["", "A", "B"],  # A -> B -> C
            }
        )
        dag = build_dag(df)

        assert "A" in dag
        assert "B" in dag
        assert "C" in dag
        assert dag["A"]["successors"] == ["B"]
        assert dag["B"]["precedents"] == ["A"]
        assert dag["B"]["successors"] == ["C"]
        assert dag["C"]["precedents"] == ["B"]

    def test_build_dag_diamond(self):
        """Test DAG building with diamond structure."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B", "C", "D"],
                "선행부재ID": [
                    "",
                    "A",
                    "A",
                    "B",
                ],  # A -> B, A -> C, B -> D (C -> D missing but okay)
            }
        )
        dag = build_dag(df)

        assert "A" in dag
        assert "B" in dag
        assert "C" in dag
        assert "D" in dag
        assert "B" in dag["A"]["successors"]
        assert "C" in dag["A"]["successors"]
        assert "D" in dag["B"]["successors"]

    def test_build_dag_empty_precedent(self):
        """Test DAG building with empty precedent IDs."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B", "C"],
                "선행부재ID": ["", "", "A"],  # A and B are roots
            }
        )
        dag = build_dag(df)

        assert dag["A"]["precedents"] == []
        assert dag["B"]["precedents"] == []
        assert dag["C"]["precedents"] == ["A"]

    def test_build_dag_nan_precedent(self):
        """Test DAG building with NaN precedent IDs."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B", "C"],
                "선행부재ID": [None, "A", "B"],  # A is root
            }
        )
        dag = build_dag(df)

        assert dag["A"]["precedents"] == []
        assert dag["B"]["precedents"] == ["A"]
        assert dag["C"]["precedents"] == ["B"]

    def test_build_dag_raises_on_cycle(self):
        """Test that build_dag raises ValueError on cycle."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B", "C"],
                "선행부재ID": ["C", "A", "B"],  # A -> B -> C -> A (cycle!)
            }
        )

        with pytest.raises(ValueError, match="Cycle detected"):
            build_dag(df)

    def test_build_dag_missing_member_column(self):
        """Test error handling for missing 부재ID column."""
        df = pd.DataFrame(
            {
                "선행부재ID": ["", "A"],
            }
        )

        with pytest.raises(ValueError, match="Missing required column: 부재ID"):
            build_dag(df)

    def test_build_dag_missing_precedent_column(self):
        """Test error handling for missing 선행부재ID column."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B"],
            }
        )

        with pytest.raises(ValueError, match="Missing required column: 선행부재ID"):
            build_dag(df)

    def test_build_dag_with_real_member_ids(self):
        """Test DAG building with realistic member IDs like 1CF001."""
        df = pd.DataFrame(
            {
                "부재ID": ["1CF001", "1CF002", "2GF001"],
                "선행부재ID": ["", "1CF001", "1CF002"],  # 1CF001 -> 1CF002 -> 2GF001
            }
        )
        dag = build_dag(df)

        assert "1CF001" in dag
        assert "1CF002" in dag
        assert "2GF001" in dag
        assert "1CF002" in dag["1CF001"]["successors"]
        assert "2GF001" in dag["1CF002"]["successors"]

    def test_build_dag_ignores_invalid_precedent(self):
        """Test that invalid precedent IDs are ignored."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B"],
                "선행부재ID": ["", "NONEXISTENT"],  # NONEXISTENT not in data
            }
        )
        dag = build_dag(df)

        # Should still work, just ignore the invalid precedent
        assert "A" in dag
        assert "B" in dag
        assert dag["B"]["precedents"] == []
