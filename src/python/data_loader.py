"""Data loader module for CSV file processing."""

import pandas as pd
from charset_normalizer import from_path

# Required columns that must exist in the CSV
REQUIRED_COLUMNS = [
    "부재ID",
    "node_i_x",
    "node_i_y",
    "node_i_z",
    "node_j_x",
    "node_j_y",
    "node_j_z",
    "선행부재ID",
]


def load_csv(filepath: str) -> pd.DataFrame:
    """Load CSV file with automatic encoding detection.

    Uses charset_normalizer to detect file encoding (UTF-8, EUC-KR, etc.)
    and loads the CSV using pandas.

    Args:
        filepath: Path to the CSV file.

    Returns:
        pandas.DataFrame: DataFrame containing CSV data.

    Raises:
        FileNotFoundError: If the specified file does not exist.
        ValueError: If required columns are missing from the CSV.

    Example:
        >>> df = load_csv("data.txt")
        >>> print(df.head())
    """
    # Detect encoding using charset_normalizer
    encoding_results = from_path(filepath)
    best_encoding = encoding_results.best().encoding

    # Read CSV with detected encoding
    df = pd.read_csv(filepath, encoding=best_encoding)

    # Validate required columns exist
    missing_columns = set(REQUIRED_COLUMNS) - set(df.columns)
    if missing_columns:
        raise ValueError(
            f"Missing required columns in CSV: {sorted(missing_columns)}. "
            f"Required columns are: {REQUIRED_COLUMNS}"
        )

    return df
