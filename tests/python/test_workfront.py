"""Tests for workfront module."""

import pandas as pd
import pytest

from src.python.precedent_graph import build_dag
from src.python.workfront import identify_workfronts, get_workfront_members


class TestIdentifyWorkfronts:
    """Tests for identify_workfronts function."""

    def test_two_workfront_starts(self):
        """Test identification of two workfront start points."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B", "C", "D"],
                "선행부재ID": ["", "", "A", "B"],  # A, B are workfront starts
            }
        )
        workfronts = identify_workfronts(df)

        assert len(workfronts) == 2
        assert workfronts[0] == (1, "A")
        assert workfronts[1] == (2, "B")

    def test_single_workfront_chain(self):
        """Test single workfront with linear chain."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B", "C"],
                "선행부재ID": ["", "A", "B"],  # Only A is workfront start
            }
        )
        workfronts = identify_workfronts(df)

        assert len(workfronts) == 1
        assert workfronts[0] == (1, "A")

    def test_workfront_id_starts_at_one(self):
        """Test that workfront_id starts at 1, not 0."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B"],
                "선행부재ID": ["", ""],
            }
        )
        workfronts = identify_workfronts(df)

        assert workfronts[0][0] == 1
        assert workfronts[1][0] == 2

    def test_sorted_by_member_id(self):
        """Test that workfronts are sorted by member_id."""
        df = pd.DataFrame(
            {
                "부재ID": ["C", "A", "B"],
                "선행부재ID": ["", "", "A"],  # A and C are workfront starts
            }
        )
        workfronts = identify_workfronts(df)

        # Should be sorted alphabetically by member_id
        assert workfronts[0] == (1, "A")
        assert workfronts[1] == (2, "C")

    def test_nan_precedent(self):
        """Test handling of NaN precedent IDs."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B", "C"],
                "선행부재ID": [None, "A", None],  # A and C are workfront starts
            }
        )
        workfronts = identify_workfronts(df)

        assert len(workfronts) == 2
        assert workfronts[0] == (1, "A")
        assert workfronts[1] == (2, "C")

    def test_empty_string_precedent(self):
        """Test handling of empty string precedent IDs."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B"],
                "선행부재ID": ["", ""],  # Both are workfront starts
            }
        )
        workfronts = identify_workfronts(df)

        assert len(workfronts) == 2

    def test_missing_member_column(self):
        """Test error handling for missing 부재ID column."""
        df = pd.DataFrame(
            {
                "선행부재ID": ["", "A"],
            }
        )

        with pytest.raises(ValueError, match="Missing required column: 부재ID"):
            identify_workfronts(df)

    def test_missing_precedent_column(self):
        """Test error handling for missing 선행부재ID column."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B"],
            }
        )

        with pytest.raises(ValueError, match="Missing required column: 선행부재ID"):
            identify_workfronts(df)

    def test_empty_dataframe(self):
        """Test error handling for empty DataFrame."""
        df = pd.DataFrame(
            {
                "부재ID": [],
                "선행부재ID": [],
            }
        )

        with pytest.raises(ValueError, match="DataFrame is empty"):
            identify_workfronts(df)

    def test_no_workfront_starts(self):
        """Test error when all members have precedents."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B"],
                "선행부재ID": ["B", "A"],  # Both have precedents
            }
        )

        with pytest.raises(ValueError, match="No workfront start points found"):
            identify_workfronts(df)

    def test_with_real_member_ids(self):
        """Test with realistic member IDs like 1CF001."""
        df = pd.DataFrame(
            {
                "부재ID": ["1CF001", "1CF002", "2GF001"],
                "선행부재ID": ["", "1CF001", ""],  # 1CF001 and 2GF001 are starts
            }
        )
        workfronts = identify_workfronts(df)

        assert len(workfronts) == 2
        assert workfronts[0] == (1, "1CF001")
        assert workfronts[1] == (2, "2GF001")

    def test_whitespace_precedent(self):
        """Test handling of whitespace-only precedent IDs."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B"],
                "선행부재ID": ["  ", "A"],  # Whitespace treated as empty
            }
        )
        workfronts = identify_workfronts(df)

        assert len(workfronts) == 1
        assert workfronts[0] == (1, "A")


class TestGetWorkfrontMembers:
    """Tests for get_workfront_members function."""

    def test_two_separate_workfronts(self):
        """Test getting members for two separate workfronts."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B", "C", "D"],
                "선행부재ID": ["", "", "A", "B"],  # A -> C, B -> D
            }
        )
        dag = build_dag(df)
        members = get_workfront_members(df, dag)

        assert len(members) == 2
        assert members[1] == ["A", "C"]
        assert members[2] == ["B", "D"]

    def test_single_workfront_chain(self):
        """Test single workfront with linear chain."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B", "C"],
                "선행부재ID": ["", "A", "B"],  # A -> B -> C
            }
        )
        dag = build_dag(df)
        members = get_workfront_members(df, dag)

        assert len(members) == 1
        assert members[1] == ["A", "B", "C"]

    def test_diamond_workfront(self):
        """Test workfront with diamond dependency structure."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B", "C", "D"],
                "선행부재ID": ["", "A", "A", "B"],  # A -> B, A -> C, B -> D
            }
        )
        dag = build_dag(df)
        members = get_workfront_members(df, dag)

        assert len(members) == 1
        # A, B, C, D all belong to workfront 1 (starting from A)
        assert members[1] == ["A", "B", "C", "D"]

    def test_multiple_workfronts_complex(self):
        """Test multiple workfronts with complex dependencies."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B", "C", "D", "E", "F"],
                "선행부재ID": ["", "", "A", "B", "C", "B"],
                # Workfront 1: A -> C -> E
                # Workfront 2: B -> D, B -> F
            }
        )
        dag = build_dag(df)
        members = get_workfront_members(df, dag)

        assert len(members) == 2
        # Workfront 1: A, C, E
        assert set(members[1]) == {"A", "C", "E"}
        # Workfront 2: B, D, F
        assert set(members[2]) == {"B", "D", "F"}

    def test_missing_member_column(self):
        """Test error handling for missing 부재ID column."""
        df = pd.DataFrame(
            {
                "선행부재ID": ["", "A"],
            }
        )
        dag = {}

        with pytest.raises(ValueError, match="Missing required column: 부재ID"):
            get_workfront_members(df, dag)

    def test_missing_precedent_column(self):
        """Test error handling for missing 선행부재ID column."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B"],
            }
        )
        dag = {}

        with pytest.raises(ValueError, match="Missing required column: 선행부재ID"):
            get_workfront_members(df, dag)

    def test_empty_dataframe(self):
        """Test error handling for empty DataFrame."""
        df = pd.DataFrame(
            {
                "부재ID": [],
                "선행부재ID": [],
            }
        )
        dag = {}

        with pytest.raises(ValueError, match="DataFrame is empty"):
            get_workfront_members(df, dag)

    def test_workfront_ids_start_at_one(self):
        """Test that workfront IDs in result start at 1."""
        df = pd.DataFrame(
            {
                "부재ID": ["A", "B", "C", "D"],
                "선행부재ID": ["", "", "A", "B"],
            }
        )
        dag = build_dag(df)
        members = get_workfront_members(df, dag)

        assert 1 in members
        assert 2 in members
        assert 0 not in members

    def test_members_sorted(self):
        """Test that members within each workfront are sorted."""
        df = pd.DataFrame(
            {
                "부재ID": ["C", "A", "B"],
                "선행부재ID": ["A", "", "A"],
            }
        )
        dag = build_dag(df)
        members = get_workfront_members(df, dag)

        # Workfront 1 should have sorted members
        assert members[1] == ["A", "B", "C"]
