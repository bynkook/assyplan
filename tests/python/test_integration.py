"""Integration tests for the complete pipeline."""

import os
import tempfile
import pytest

from src.python.data_loader import load_csv
from src.python.node_table import create_node_table
from src.python.element_table import create_element_table
from src.python.validators import validate_all


def get_data_file():
    """Return path to the sample data file."""
    # Get project root by going up from tests/python/
    current_dir = os.path.dirname(os.path.abspath(__file__))
    project_root = os.path.dirname(os.path.dirname(current_dir))
    return os.path.join(project_root, "data.txt")


class TestIntegration:
    """Integration tests for the complete pipeline."""

    def test_e2e_pipeline(self):
        """Test complete pipeline: load → node table → element table → validation."""
        data_file = get_data_file()

        # Load CSV data
        df = load_csv(data_file)
        assert len(df) == 259, f"Expected 259 rows, got {len(df)}"

        # Create node table
        nodes = create_node_table(df)
        assert len(nodes) > 0, "Node table should not be empty"
        assert nodes[0][0] == 1, "First node ID should start from 1"

        # Verify node coordinates
        first_node = nodes[0]
        assert len(first_node) == 4, "Node tuple should have (id, x, y, z)"
        assert isinstance(first_node[1], (int, float)), "x coordinate should be numeric"
        assert isinstance(first_node[2], (int, float)), "y coordinate should be numeric"
        assert isinstance(first_node[3], (int, float)), "z coordinate should be numeric"

        # Create element table
        elements = create_element_table(df, nodes)
        assert len(elements) == 259, f"Expected 259 elements, got {len(elements)}"

        # Verify element structure
        first_element = elements[0]
        assert len(first_element) == 4, (
            "Element tuple should have (id, node_i, node_j, type)"
        )
        assert first_element[0] == 1, "First element ID should start from 1"
        assert first_element[3] in ("Column", "Girder"), (
            "Member type should be Column or Girder"
        )

        # Validate all
        assert validate_all(nodes, elements) is True

    def test_load_csv_data(self):
        """Test loading the sample data file."""
        df = load_csv(get_data_file())

        # Check required columns exist
        required_cols = [
            "부재ID",
            "node_i_x",
            "node_i_y",
            "node_i_z",
            "node_j_x",
            "node_j_y",
            "node_j_z",
            "선행부재ID",
        ]
        for col in required_cols:
            assert col in df.columns, f"Missing required column: {col}"

    def test_create_node_table_from_data(self):
        """Test node table creation from DataFrame."""
        df = load_csv(get_data_file())
        nodes = create_node_table(df)

        # Verify sorted order (x → y → z ascending)
        for i in range(len(nodes) - 1):
            x1, y1, z1 = nodes[i][1], nodes[i][2], nodes[i][3]
            x2, y2, z2 = nodes[i + 1][1], nodes[i + 1][2], nodes[i + 1][3]

            if x1 < x2:
                continue
            elif x1 == x2:
                if y1 < y2:
                    continue
                elif y1 == y2:
                    assert z1 < z2, f"Nodes not sorted by z at index {i}"

    def test_create_element_table_with_nodes(self):
        """Test element table creation with node mapping."""
        df = load_csv(get_data_file())
        nodes = create_node_table(df)
        elements = create_element_table(df, nodes)

        # All node IDs in elements should exist in node table
        node_ids = {n[0] for n in nodes}
        for elem_id, node_i_id, node_j_id, _ in elements:
            assert node_i_id in node_ids, (
                f"Element {elem_id}: node_i_id {node_i_id} not in nodes"
            )
            assert node_j_id in node_ids, (
                f"Element {elem_id}: node_j_id {node_j_id} not in nodes"
            )

    def test_validate_all_data(self):
        """Test validation of nodes and elements."""
        df = load_csv(get_data_file())
        nodes = create_node_table(df)
        elements = create_element_table(df, nodes)

        # Should pass validation for valid data
        assert validate_all(nodes, elements) is True


class TestErrorCases:
    """Error case tests for the pipeline."""

    def test_missing_columns(self):
        """Test error handling for missing columns."""
        # Create CSV with missing required columns
        csv_content = "부재ID,node_i_x,node_i_y\n1,0,0\n"

        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".csv", delete=False, encoding="utf-8"
        ) as f:
            f.write(csv_content)
            temp_path = f.name

        try:
            with pytest.raises(ValueError, match="Missing required columns"):
                load_csv(temp_path)
        finally:
            os.unlink(temp_path)

    def test_invalid_csv(self):
        """Test handling of invalid CSV file."""
        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".csv", delete=False, encoding="utf-8"
        ) as f:
            f.write("not,a,valid,csv\n")
            temp_path = f.name

        try:
            with pytest.raises(Exception):  # pandas or charset_normalizer may raise
                load_csv(temp_path)
        finally:
            os.unlink(temp_path)

    def test_nonexistent_file(self):
        """Test handling of non-existent file."""
        with pytest.raises(FileNotFoundError):
            load_csv("nonexistent_file.csv")

    def test_validation_fails_on_invalid_axis(self):
        """Test that validation catches non-axis-parallel elements."""
        import pandas as pd

        # Create data with diagonal element (not axis-parallel)
        df = pd.DataFrame(
            {
                "부재ID": [1],
                "node_i_x": [0.0],
                "node_i_y": [0.0],
                "node_i_z": [0.0],
                "node_j_x": [1.0],
                "node_j_y": [1.0],
                "node_j_z": [0.0],  # diagonal in xy plane
                "선행부재ID": [""],
            }
        )

        nodes = create_node_table(df)
        elements = create_element_table(df, nodes)

        # Should raise ValueError because element is not axis-parallel
        with pytest.raises(ValueError, match="not axis-parallel"):
            validate_all(nodes, elements)

    def test_validation_fails_on_zero_length_element(self):
        """Test that validation catches zero-length elements."""
        import pandas as pd

        # Create data with zero-length element (same start and end node)
        df = pd.DataFrame(
            {
                "부재ID": [1],
                "node_i_x": [0.0],
                "node_i_y": [0.0],
                "node_i_z": [0.0],
                "node_j_x": [0.0],
                "node_j_y": [0.0],
                "node_j_z": [0.0],  # same as node_i
                "선행부재ID": [""],
            }
        )

        nodes = create_node_table(df)
        elements = create_element_table(df, nodes)

        # Should raise ValueError because element has zero length
        with pytest.raises(ValueError, match="Zero-length element"):
            validate_all(nodes, elements)

    def test_validation_fails_on_duplicate_node_id(self):
        """Test that validation catches duplicate node IDs."""
        import pandas as pd
        from src.python.node_table import create_node_table

        # Create nodes manually with duplicate IDs
        nodes = [
            (1, 0.0, 0.0, 0.0),
            (1, 1.0, 0.0, 0.0),  # duplicate ID
            (2, 1.0, 1.0, 0.0),
        ]

        # Create simple elements
        elements = [
            (1, 1, 2, "Column"),
            (2, 2, 3, "Girder"),
        ]

        # Should raise ValueError because of duplicate node IDs
        with pytest.raises(ValueError, match="Duplicate node IDs"):
            validate_all(nodes, elements)
