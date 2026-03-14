"""Encoding detection utilities using charset_normalizer."""

from charset_normalizer import from_path


def detect_encoding(filepath: str) -> str:
    """Detect the encoding of a file.

    Args:
        filepath: Path to the file to detect encoding for.

    Returns:
        Encoding name as string (e.g., 'utf-8', 'EUC-KR').

    Raises:
        FileNotFoundError: If the file does not exist.

    Example:
        >>> encoding = detect_encoding('data.txt')
        >>> print(encoding)
        utf-8
    """
    result = from_path(filepath).best()
    if result is None:
        return "utf-8"  # Default fallback
    return result.encoding
