import json
import os
import socket
import subprocess
import sys
import time
from pathlib import Path

import pytest
import requests

ROOT_DIR = Path(__file__).resolve().parents[2]
APEX_BIN = ROOT_DIR / "target" / "debug" / "apex"
RUNTIME_DIR = ROOT_DIR / ".run" / "e2e" / "router_strategy"
CONFIG_PATH = RUNTIME_DIR / "temp_router_config.json"
SERVER_LOG = RUNTIME_DIR / "router_strategy_server.log"
MOCK_SERVER_SCRIPT = Path(__file__).parent / "mock_server.py"


def wait_for_server(host: str, port: int, timeout: int = 10) -> bool:
    start = time.time()
    while time.time() - start < timeout:
        try:
            with socket.create_connection((host, port), timeout=1):
                return True
        except OSError:
            time.sleep(0.2)
    return False


@pytest.fixture(scope="module")
def mock_servers():
    proc_a = subprocess.Popen(
        [sys.executable, str(MOCK_SERVER_SCRIPT), "--port", "8081", "--id", "A"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    proc_b = subprocess.Popen(
        [sys.executable, str(MOCK_SERVER_SCRIPT), "--port", "8082", "--id", "B"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    if not wait_for_server("127.0.0.1", 8081) or not wait_for_server("127.0.0.1", 8082):
        proc_a.terminate()
        proc_b.terminate()
        pytest.fail("mock servers failed to start")

    yield

    proc_a.terminate()
    proc_b.terminate()


@pytest.fixture(scope="module")
def apex_server(mock_servers):
    config = {
        "version": "1.0",
        "global": {
            "listen": "127.0.0.1:18080",
            "auth_keys": [],
            "timeouts": {
                "connect_ms": 1000,
                "request_ms": 5000,
                "response_ms": 5000,
            },
            "retries": {
                "max_attempts": 1,
                "backoff_ms": 100,
                "retry_on_status": [429, 500, 502, 503, 504],
            },
            "cors_allowed_origins": [],
        },
        "logging": {"level": "info", "dir": None},
        "data_dir": str((RUNTIME_DIR / "data").resolve()),
        "web_dir": "target/web",
        "channels": [
            {
                "name": "channel_a",
                "provider_type": "openai",
                "base_url": "http://127.0.0.1:8081",
                "api_key": "dummy",
            },
            {
                "name": "channel_b",
                "provider_type": "openai",
                "base_url": "http://127.0.0.1:8082",
                "api_key": "dummy",
            },
        ],
        "routers": [
            {
                "name": "round-robin-router",
                "channels": [],
                "strategy": "round_robin",
                "fallback_channels": [],
                "rules": [
                    {
                        "match": {"models": ["rr-*"]},
                        "channels": [
                            {"name": "channel_a", "weight": 1},
                            {"name": "channel_b", "weight": 1},
                        ],
                        "strategy": "round_robin",
                    }
                ],
            },
            {
                "name": "priority-router",
                "channels": [],
                "strategy": "priority",
                "fallback_channels": [],
                "rules": [
                    {
                        "match": {"models": ["priority-*"]},
                        "channels": [
                            {"name": "channel_a", "weight": 1},
                            {"name": "channel_b", "weight": 1},
                        ],
                        "strategy": "priority",
                    }
                ],
            },
            {
                "name": "model-match-router",
                "channels": [],
                "strategy": "priority",
                "fallback_channels": [],
                "rules": [
                    {
                        "match": {"models": ["gpt-4"]},
                        "channels": [{"name": "channel_a", "weight": 1}],
                        "strategy": "priority",
                    },
                    {
                        "match": {"models": ["model-*"]},
                        "channels": [{"name": "channel_b", "weight": 1}],
                        "strategy": "priority",
                    },
                ],
            },
        ],
        "metrics": {"enabled": False, "path": "/metrics"},
        "hot_reload": {
            "config_path": str(CONFIG_PATH),
            "watch": False,
        },
        "teams": [
            {
                "id": "router-test-team",
                "api_key": "sk-router-test",
                "policy": {
                    "allowed_routers": [
                        "round-robin-router",
                        "priority-router",
                        "model-match-router",
                    ],
                    "allowed_models": ["rr-*", "priority-*", "gpt-4", "model-*"],
                },
            }
        ],
        "compliance": None,
    }

    CONFIG_PATH.parent.mkdir(parents=True, exist_ok=True)
    with open(CONFIG_PATH, "w") as f:
        json.dump(config, f, indent=2)

    with open(SERVER_LOG, "w") as log_file:
        proc = subprocess.Popen(
            [
                str(APEX_BIN),
                "gateway",
                "start",
                "--config",
                str(CONFIG_PATH),
            ],
            cwd=ROOT_DIR,
            stdout=log_file,
            stderr=subprocess.STDOUT,
            text=True,
        )

    if not wait_for_server("127.0.0.1", 18080):
        proc.terminate()
        logs = SERVER_LOG.read_text() if SERVER_LOG.exists() else ""
        pytest.fail(f"Apex failed to start:\n{logs}")

    yield "http://127.0.0.1:18080"

    proc.terminate()
    try:
        proc.wait(timeout=3)
    except subprocess.TimeoutExpired:
        proc.kill()

    if CONFIG_PATH.exists():
        CONFIG_PATH.unlink()


def send_request(base_url: str, model: str):
    headers = {
        "Authorization": "Bearer sk-router-test",
        "Content-Type": "application/json",
    }
    data = {
        "model": model,
        "messages": [{"role": "user", "content": "Hello"}],
    }
    response = requests.post(
        f"{base_url}/v1/chat/completions", headers=headers, json=data, timeout=5
    )
    assert response.status_code == 200, response.text
    return response.json()


def test_priority_strategy(apex_server):
    for _ in range(5):
        resp = send_request(apex_server, "priority-test")
        content = resp["choices"][0]["message"]["content"]
        assert "Response from A" in content


def test_round_robin_strategy(apex_server):
    responses = []
    for _ in range(12):
        resp = send_request(apex_server, "rr-test")
        responses.append(resp["choices"][0]["message"]["content"])

    a_count = sum(1 for r in responses if "Response from A" in r)
    b_count = sum(1 for r in responses if "Response from B" in r)

    assert a_count > 0
    assert b_count > 0


def test_model_matching(apex_server):
    resp = send_request(apex_server, "gpt-4")
    assert "Response from A" in resp["choices"][0]["message"]["content"]

    resp = send_request(apex_server, "model-test")
    assert "Response from B" in resp["choices"][0]["message"]["content"]
