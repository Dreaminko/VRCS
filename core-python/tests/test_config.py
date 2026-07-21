from app.config import AppConfig, load_config, save_config


def test_config_round_trip(tmp_path):
    path = tmp_path / "config.json"
    expected = AppConfig(port=9000)
    save_config(path, expected)
    assert load_config(path) == expected


def test_missing_config_is_created(tmp_path):
    path = tmp_path / "config.json"
    assert load_config(path) == AppConfig()
    assert path.exists()

