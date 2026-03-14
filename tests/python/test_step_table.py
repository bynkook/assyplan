"""Tests for step_table module."""

import json
import os
import tempfile

import pandas as pd
import pytest

from src.python.step_table import (
    assign_steps,
    create_step_table,
    save_step_table,
)


class TestAssignSteps:
    """Tests for assign_steps function."""

    def test_minimum_assembly_all_step_1(self):
        """Test that minimum assembly (3 columns + 2 girders) is step 1."""
        nodes = [
            (1, 0.0, 0.0, 0.0),
            (2, 0.0, 0.0, 3.0),
            (3, 1.0, 0.0, 0.0),
            (4, 1.0, 0.0, 3.0),
            (5, 1.0, 1.0, 0.0),
            (6, 1.0, 1.0, 3.0),
        ]
        elements = [
            (1, 1, 2, "Column"),
            (2, 3, 4, "Column"),
            (3, 5, 6, "Column"),
            (4, 2, 4, "Girder"),
            (5, 4, 6, "Girder"),
        ]
        sequence = [(1, "A"), (1, "B"), (1, "C"), (1, "D"), (1, "E")]
        dag = {}

        result = assign_steps(sequence, nodes, elements, dag)

        # All members should be step 1 (minimum assembly is stable)
        assert len(result) == 5
        for wf_id, step, member_id in result:
            assert step == 1, f"Expected step 1, got {step}"

    def test_multiple_workfronts_independent(self):
        """Test that multiple workfronts are processed independently."""
        nodes = [
            (1, 0.0, 0.0, 0.0),
            (2, 0.0, 0.0, 3.0),
            (3, 1.0, 0.0, 0.0),
            (4, 1.0, 0.0, 3.0),
            (5, 1.0, 1.0, 0.0),
            (6, 1.0, 1.0, 3.0),
        ]
        elements = [
            (1, 1, 2, "Column"),
            (2, 3, 4, "Column"),
            (3, 5, 6, "Column"),
            (4, 2, 4, "Girder"),
            (5, 4, 6, "Girder"),
        ]
        sequence = [(1, "A"), (1, "B"), (2, "C"), (2, "D")]
        dag = {}

        result = assign_steps(sequence, nodes, elements, dag)

        assert len(result) == 4
        # Each workfront should have its own step starting from 1
        wf1_members = [(wf, step, m) for wf, step, m in result if wf == 1]
        wf2_members = [(wf, step, m) for wf, step, m in result if wf == 2]
        for _, step, _ in wf1_members:
            assert step == 1
        for _, step, _ in wf2_members:
            assert step == 1

    def test_empty_sequence_raises_error(self):
        """Test that empty sequence raises ValueError."""
        nodes = [(1, 0.0, 0.0, 0.0), (2, 0.0, 0.0, 3.0)]
        elements = [(1, 1, 2, "Column")]
        dag = {}

        with pytest.raises(ValueError, match="Sequence cannot be empty"):
            assign_steps([], nodes, elements, dag)

    def test_empty_nodes_raises_error(self):
        """Test that empty nodes raises ValueError."""
        sequence = [(1, "A")]
        elements = [(1, 1, 2, "Column")]
        dag = {}

        with pytest.raises(ValueError, match="Nodes cannot be empty"):
            assign_steps(sequence, [], elements, dag)

    def test_empty_elements_raises_error(self):
        """Test that empty elements raises ValueError."""
        sequence = [(1, "A")]
        nodes = [(1, 0.0, 0.0, 0.0), (2, 0.0, 0.0, 3.0)]
        dag = {}

        with pytest.raises(ValueError, match="Elements cannot be empty"):
            assign_steps(sequence, nodes, [], dag)

    def test_step_starts_from_1(self):
        """Test that step starts from 1, not 0."""
        nodes = [
            (1, 0.0, 0.0, 0.0),
            (2, 0.0, 0.0, 3.0),
            (3, 1.0, 0.0, 0.0),
            (4, 1.0, 0.0, 3.0),
            (5, 1.0, 1.0, 0.0),
            (6, 1.0, 1.0, 3.0),
        ]
        elements = [
            (1, 1, 2, "Column"),
            (2, 3, 4, "Column"),
            (3, 5, 6, "Column"),
            (4, 2, 4, "Girder"),
            (5, 4, 6, "Girder"),
        ]
        sequence = [(1, "A")]
        dag = {}

        result = assign_steps(sequence, nodes, elements, dag)

        # Verify step is 1, not 0
        assert result[0][1] == 1

    def test_numeric_member_ids(self):
        """Test that numeric member IDs work correctly."""
        nodes = [
            (1, 0.0, 0.0, 0.0),
            (2, 0.0, 0.0, 3.0),
            (3, 1.0, 0.0, 0.0),
            (4, 1.0, 0.0, 3.0),
            (5, 1.0, 1.0, 0.0),
            (6, 1.0, 1.0, 3.0),
        ]
        elements = [
            (1, 1, 2, "Column"),
            (2, 3, 4, "Column"),
            (3, 5, 6, "Column"),
            (4, 2, 4, "Girder"),
            (5, 4, 6, "Girder"),
        ]
        sequence = [(1, "1"), (1, "2"), (1, "3")]
        dag = {}

        result = assign_steps(sequence, nodes, elements, dag)

        assert len(result) == 3


class TestCreateStepTable:
    """Tests for create_step_table function."""

    def test_wrapper_returns_correct_format(self):
        """Test that create_step_table returns correct format."""
        nodes = [
            (1, 0.0, 0.0, 0.0),
            (2, 0.0, 0.0, 3.0),
            (3, 1.0, 0.0, 0.0),
            (4, 1.0, 0.0, 3.0),
            (5, 1.0, 1.0, 0.0),
            (6, 1.0, 1.0, 3.0),
        ]
        elements = [
            (1, 1, 2, "Column"),
            (2, 3, 4, "Column"),
            (3, 5, 6, "Column"),
            (4, 2, 4, "Girder"),
            (5, 4, 6, "Girder"),
        ]
        sequence = [(1, "A"), (1, "B")]
        dag = {}

        result = create_step_table(sequence, nodes, elements, dag)

        # Should return list of tuples with 3 elements
        assert len(result) == 2
        for item in result:
            assert len(item) == 3
            assert isinstance(item[0], int)  # workfront_id
            assert isinstance(item[1], int)  # step
            assert isinstance(item[2], str)  # member_id

    def test_empty_sequence_raises_error(self):
        """Test that empty sequence raises ValueError."""
        nodes = [(1, 0.0, 0.0, 0.0)]
        elements = [(1, 1, 2, "Column")]
        dag = {}

        with pytest.raises(ValueError, match="Sequence cannot be empty"):
            create_step_table([], nodes, elements, dag)


class TestSaveStepTable:
    """Tests for save_step_table function."""

    def test_save_csv(self):
        """Test saving step table to CSV."""
        step_table = [(1, 1, "A"), (1, 1, "B"), (2, 1, "C")]

        with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
            filepath = f.name

        try:
            save_step_table(step_table, filepath, format="csv")

            df = pd.read_csv(filepath)
            assert len(df) == 3
            assert df.columns.tolist() == ["workfront_id", "step", "member_id"]
            assert df["workfront_id"].tolist() == [1, 1, 2]
            assert df["step"].tolist() == [1, 1, 1]
            assert df["member_id"].tolist() == ["A", "B", "C"]
        finally:
            os.unlink(filepath)

    def test_save_json(self):
        """Test saving step table to JSON."""
        step_table = [(1, 1, "A"), (1, 2, "B"), (2, 1, "C")]

        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            filepath = f.name

        try:
            save_step_table(step_table, filepath, format="json")

            with open(filepath, "r", encoding="utf-8") as f:
                data = json.load(f)

            assert len(data) == 3
            assert data[0] == {"workfront_id": 1, "step": 1, "member_id": "A"}
            assert data[1] == {"workfront_id": 1, "step": 2, "member_id": "B"}
            assert data[2] == {"workfront_id": 2, "step": 1, "member_id": "C"}
        finally:
            os.unlink(filepath)

    def test_empty_step_table_raises_error(self):
        """Test that empty step table raises ValueError."""
        with pytest.raises(ValueError, match="Step table cannot be empty"):
            save_step_table([], "test.csv")

    def test_unsupported_format_raises_error(self):
        """Test that unsupported format raises ValueError."""
        step_table = [(1, 1, "A")]

        with pytest.raises(ValueError, match="Unsupported format"):
            save_step_table(step_table, "test.txt", format="txt")

    def test_csv_format_case_insensitive(self):
        """Test that CSV format is case insensitive."""
        step_table = [(1, 1, "A"), (2, 1, "B")]

        with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
            filepath = f.name

        try:
            save_step_table(step_table, filepath, format="CSV")
            df = pd.read_csv(filepath)
            assert len(df) == 2
        finally:
            os.unlink(filepath)

    def test_json_format_case_insensitive(self):
        """Test that JSON format is case insensitive."""
        step_table = [(1, 1, "A"), (2, 1, "B")]

        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            filepath = f.name

        try:
            save_step_table(step_table, filepath, format="Json")
            with open(filepath, "r", encoding="utf-8") as f:
                data = json.load(f)
            assert len(data) == 2
        finally:
            os.unlink(filepath)


class TestIntegration:
    """Integration tests combining all functions."""

    def test_full_workflow(self):
        """Test full workflow from sequence to saved file."""
        nodes = [
            (1, 0.0, 0.0, 0.0),
            (2, 0.0, 0.0, 3.0),
            (3, 1.0, 0.0, 0.0),
            (4, 1.0, 0.0, 3.0),
            (5, 1.0, 1.0, 0.0),
            (6, 1.0, 1.0, 3.0),
        ]
        elements = [
            (1, 1, 2, "Column"),
            (2, 3, 4, "Column"),
            (3, 5, 6, "Column"),
            (4, 2, 4, "Girder"),
            (5, 4, 6, "Girder"),
        ]
        sequence = [(1, "A"), (1, "B"), (1, "C"), (1, "D"), (1, "E")]
        dag = {}

        # Create step table
        step_table = create_step_table(sequence, nodes, elements, dag)
        assert len(step_table) == 5

        # Save to CSV
        with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False) as f:
            csv_path = f.name

        try:
            save_step_table(step_table, csv_path, format="csv")
            df_result = pd.read_csv(csv_path)
            assert len(df_result) == 5
            assert "workfront_id" in df_result.columns
            assert "step" in df_result.columns
            assert "member_id" in df_result.columns
        finally:
            os.unlink(csv_path)

        # Save to JSON
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            json_path = f.name

        try:
            save_step_table(step_table, json_path, format="json")
            with open(json_path, "r", encoding="utf-8") as f:
                data = json.load(f)
            assert len(data) == 5
            assert all("workfront_id" in item for item in data)
            assert all("step" in item for item in data)
            assert all("member_id" in item for item in data)
        finally:
            os.unlink(json_path)

    def test_multiple_workfronts_full_flow(self):
        """Test multiple workfronts with complete flow."""
        nodes = [
            (1, 0.0, 0.0, 0.0),
            (2, 0.0, 0.0, 3.0),
            (3, 1.0, 0.0, 0.0),
            (4, 1.0, 0.0, 3.0),
            (5, 1.0, 1.0, 0.0),
            (6, 1.0, 1.0, 3.0),
        ]
        elements = [
            (1, 1, 2, "Column"),
            (2, 3, 4, "Column"),
            (3, 5, 6, "Column"),
            (4, 2, 4, "Girder"),
            (5, 4, 6, "Girder"),
            (6, 7, 8, "Column"),
            (7, 8, 9, "Girder"),
        ]
        sequence = [
            (1, "A"),
            (1, "B"),
            (1, "C"),
            (2, "D"),
            (2, "E"),
        ]
        dag = {}

        step_table = create_step_table(sequence, nodes, elements, dag)

        # Verify structure
        assert len(step_table) == 5
        workfront_1 = [(wf, step, m) for wf, step, m in step_table if wf == 1]
        workfront_2 = [(wf, step, m) for wf, step, m in step_table if wf == 2]

        # Both workfronts should have step 1 (independent)
        for _, step, _ in workfront_1:
            assert step == 1
        for _, step, _ in workfront_2:
            assert step == 1
