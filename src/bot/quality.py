"""Inline quality-picker choices and their mapping to download parameters."""

from typing import Optional, Tuple

from ..types.download import MediaFormat

# (button label, callback value). Order = display order.
QUALITY_CHOICES = [
    ("🎬 Best", "best"),
    ("1080p", "1080"),
    ("720p", "720"),
    ("480p", "480"),
    ("🎵 Audio (MP3)", "audio"),
]

_VALID = {value for _, value in QUALITY_CHOICES}


def is_valid_choice(value: str) -> bool:
    return value in _VALID


def quality_params(value: str) -> Tuple[MediaFormat, Optional[int]]:
    """Map a picker choice to (preferred_format, max_height).

    A height cap that exceeds what's available just falls back to the best
    available stream, so every choice is always valid.
    """
    if value == "audio":
        return MediaFormat.AUDIO, None
    if value == "best":
        return MediaFormat.VIDEO, None
    if value.isdigit():
        return MediaFormat.VIDEO, int(value)
    return MediaFormat.AUTO, None
