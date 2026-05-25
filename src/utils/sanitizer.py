"""Filename sanitization utilities."""

import re
import unicodedata
from pathlib import Path


def sanitize_filename(filename: str, max_length: int = 200) -> str:
    """
    Sanitize a filename to prevent path traversal and invalid characters.

    Args:
        filename: Original filename
        max_length: Maximum allowed length

    Returns:
        Sanitized filename safe for filesystem use
    """
    if not filename:
        return "untitled"

    # Normalize unicode characters
    filename = unicodedata.normalize("NFKC", filename)

    # Remove path separators and null bytes
    filename = filename.replace("/", "_").replace("\\", "_").replace("\0", "")

    # Remove or replace problematic characters
    filename = re.sub(r'[<>:"|?*]', "_", filename)

    # Remove leading/trailing dots and spaces
    filename = filename.strip(". ")

    # Replace multiple spaces/underscores with single
    filename = re.sub(r"[\s_]+", "_", filename)

    # Remove any remaining dangerous patterns
    filename = filename.replace("..", "")

    # Ensure not empty
    if not filename:
        filename = "untitled"

    # Truncate if too long (preserve extension if possible)
    if len(filename) > max_length:
        name, ext = Path(filename).stem, Path(filename).suffix
        max_name_len = max_length - len(ext)
        filename = filename[:max_name_len] + ext

    return filename


def sanitize_path(base_path: Path, user_input: str) -> Path:
    """
    Safely resolve a path relative to base_path, preventing traversal.

    Args:
        base_path: Base directory that should be the root
        user_input: User-provided path component

    Returns:
        Resolved path guaranteed to be under base_path

    Raises:
        ValueError: If path would escape base_path
    """
    base_path = base_path.resolve()

    # Sanitize the input component
    sanitized = sanitize_filename(user_input)

    # Join and resolve
    target_path = (base_path / sanitized).resolve()

    # Verify it's still under base_path
    try:
        target_path.relative_to(base_path)
    except ValueError:
        raise ValueError(f"Path traversal attempt detected: {user_input}")

    return target_path


def get_safe_temp_dir(task_id: str, base_temp: Path) -> Path:
    """
    Create a safe temporary directory for a task.

    Args:
        task_id: Unique task identifier
        base_temp: Base temporary directory

    Returns:
        Path to temporary directory for this task
    """
    # Sanitize task_id to prevent any injection
    safe_id = re.sub(r"[^a-zA-Z0-9\-]", "", task_id)
    if not safe_id:
        safe_id = "unknown"

    temp_dir = base_temp / safe_id
    temp_dir.mkdir(parents=True, exist_ok=True)
    return temp_dir
