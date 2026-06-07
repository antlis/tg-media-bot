"""Tests for the file_id media cache and the message extractor."""

import json
from types import SimpleNamespace

from src.services.media_cache import MediaCache
from src.services.uploader import cache_entry_from_message


class TestMediaCache:
    def test_get_miss(self):
        c = MediaCache("")
        assert c.get("https://x", "audio") is None

    def test_put_get_roundtrip(self):
        c = MediaCache("")
        c.put("https://x", "audio", {"kind": "audio", "file_id": "AAA"})
        assert c.get("https://x", "audio")["file_id"] == "AAA"

    def test_format_is_part_of_key(self):
        c = MediaCache("")
        c.put("https://x", "audio", {"file_id": "A"})
        assert c.get("https://x", "video") is None  # different format → miss

    def test_evict(self):
        c = MediaCache("")
        c.put("https://x", "audio", {"file_id": "A"})
        c.evict("https://x", "audio")
        assert c.get("https://x", "audio") is None

    def test_persistence_roundtrip(self, tmp_path):
        path = tmp_path / "sub" / "cache.json"
        c = MediaCache(str(path))
        c.put("https://x", "video", {"kind": "video", "file_id": "VID"})
        # A fresh instance reads it back from disk
        assert MediaCache(str(path)).get("https://x", "video")["file_id"] == "VID"

    def test_corrupt_file_ignored(self, tmp_path):
        path = tmp_path / "cache.json"
        path.write_text("{bad json")
        assert MediaCache(str(path)).get("https://x", "audio") is None


class TestCacheEntryFromMessage:
    def test_audio(self):
        msg = SimpleNamespace(
            audio=SimpleNamespace(file_id="A1", title="Song", performer="Artist", duration=210),
            video=None, document=None,
        )
        e = cache_entry_from_message(msg)
        assert e == {"kind": "audio", "file_id": "A1", "title": "Song",
                     "performer": "Artist", "duration": 210}

    def test_video(self):
        msg = SimpleNamespace(audio=None,
                              video=SimpleNamespace(file_id="V1", duration=99),
                              document=None)
        e = cache_entry_from_message(msg)
        assert e["kind"] == "video" and e["file_id"] == "V1" and e["duration"] == 99

    def test_document(self):
        msg = SimpleNamespace(audio=None, video=None,
                              document=SimpleNamespace(file_id="D1"))
        assert cache_entry_from_message(msg)["kind"] == "document"

    def test_nothing_cacheable(self):
        msg = SimpleNamespace(audio=None, video=None, document=None)
        assert cache_entry_from_message(msg) is None
