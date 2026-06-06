"""Tests for configuration loading and parsing."""

import pytest

from src.config.settings import Settings, _parse_user_ids


class TestParseUserIds:
    def test_empty_string_is_open(self):
        assert _parse_user_ids("") == set()

    def test_single_id(self):
        assert _parse_user_ids("418870313") == {418870313}

    def test_multiple_ids(self):
        assert _parse_user_ids("418870313,6523467794") == {418870313, 6523467794}

    def test_whitespace_and_blanks_ignored(self):
        assert _parse_user_ids(" 1 , 2 ,, 3 ") == {1, 2, 3}

    def test_non_numeric_raises(self):
        with pytest.raises(ValueError):
            _parse_user_ids("not-a-number")


class TestSettings:
    def test_bot_token_required(self, tmp_path):
        with pytest.raises(ValueError):
            Settings(bot_token="", temp_dir=tmp_path)

    def test_temp_dir_created(self, tmp_path):
        target = tmp_path / "new" / "nested"
        s = Settings(bot_token="x", temp_dir=target)
        assert target.exists()
        assert s.temp_dir == target

    def test_log_level_normalized(self, tmp_path):
        s = Settings(bot_token="x", temp_dir=tmp_path, log_level="debug")
        assert s.log_level == "DEBUG"

    def test_allowed_users_default_empty(self, tmp_path):
        s = Settings(bot_token="x", temp_dir=tmp_path)
        assert s.allowed_users == set()

    def test_upload_limit_standard_api(self, tmp_path):
        s = Settings(bot_token="x", temp_dir=tmp_path)
        assert s.upload_limit_mb == 50

    def test_upload_limit_local_api(self, tmp_path):
        s = Settings(bot_token="x", temp_dir=tmp_path,
                     api_server_url="http://telegram-bot-api:8081")
        assert s.upload_limit_mb == 2000


class TestLoadSettings:
    def test_allowed_users_loaded_from_env(self, monkeypatch, tmp_path, reload_settings):
        monkeypatch.setenv("BOT_TOKEN", "abc")
        monkeypatch.setenv("TEMP_DIR", str(tmp_path))
        monkeypatch.setenv("ALLOWED_USERS", "418870313,6523467794")
        s = reload_settings()
        assert s.allowed_users == {418870313, 6523467794}

    def test_api_server_url_loaded(self, monkeypatch, tmp_path, reload_settings):
        monkeypatch.setenv("BOT_TOKEN", "abc")
        monkeypatch.setenv("TEMP_DIR", str(tmp_path))
        monkeypatch.setenv("API_SERVER_URL", "http://telegram-bot-api:8081")
        s = reload_settings()
        assert s.api_server_url == "http://telegram-bot-api:8081"

    def test_cookies_and_chats_files_loaded(self, monkeypatch, tmp_path, reload_settings):
        monkeypatch.setenv("BOT_TOKEN", "abc")
        monkeypatch.setenv("TEMP_DIR", str(tmp_path))
        monkeypatch.setenv("COOKIES_FILE", "/cookies/cookies.txt")
        monkeypatch.setenv("ALLOWED_CHATS_FILE", "/data/allowed_chats.json")
        s = reload_settings()
        assert s.cookies_file == "/cookies/cookies.txt"
        assert s.allowed_chats_file == "/data/allowed_chats.json"
