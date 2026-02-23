import os
import sys
import time
import json
import subprocess
import requests
import pytest
from pathlib import Path

# Paths
ROOT_DIR = Path(__file__).parent.parent.parent
APEX_BIN = ROOT_DIR / "target" / "debug" / "apex"
MOCK_SERVER_SCRIPT = Path(__file__).parent / "mock_server.py"

@pytest.fixture(scope="module")
def mock_servers():
    # Start Mock Server A
    proc_a = subprocess.Popen(
        [sys.executable, str(MOCK_SERVER_SCRIPT), "--port", "8081", "--id", "A"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE
    )
    # Start Mock Server B
    proc_b = subprocess.Popen(
        [sys.executable, str(MOCK_SERVER_SCRIPT), "--port", "8082", "--id", "B"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE
    )
    
    time.sleep(1) # Wait for servers to start
    
    yield
    
    proc_a.terminate()
    proc_b.terminate()

@pytest.fixture(scope="module")
def apex_server(mock_servers):
    # Create Config
    config = {
        "version": "1",
        "global": {
                "listen": "127.0.0.1:8080",
                "mode": "proxy",
                "log_level": "info",
                "auth": {
                    "mode": "none"
                },
                "timeouts": {
                    "connect_ms": 1000,
                    "request_ms": 5000,
                    "response_ms": 5000
                },
                "retries": {
                    "max_attempts": 1,
                    "backoff_ms": 100,
                    "retry_on_status": [429, 500, 502, 503, 504]
                }
        },
        "channels": [
            {
                    "name": "channel_a",
                    "provider_type": "openai", # Use openai type as generic http client
                    "base_url": "http://127.0.0.1:8081",
                    "api_key": "dummy",
                    "timeouts": {
                        "connect_ms": 1000,
                        "request_ms": 5000,
                        "response_ms": 5000
                    }
                },
                {
                    "name": "channel_b",
                    "provider_type": "openai",
                    "base_url": "http://127.0.0.1:8082",
                    "api_key": "dummy",
                    "timeouts": {
                        "connect_ms": 1000,
                        "request_ms": 5000,
                        "response_ms": 5000
                    }
                }
        ],
        "routers": [
            {
                "name": "round-robin-router",
                "vkey": "rr-key",
                "channels": [],
                "strategy": "round_robin",
                "rules": [
                    {
                        "match": {"model": "*"},
                        "channels": [
                            {"name": "channel_a", "weight": 1},
                            {"name": "channel_b", "weight": 1}
                        ],
                        "strategy": "round_robin"
                    }
                ]
            },
            {
                "name": "priority-router",
                "vkey": "p-key",
                "channels": [],
                "strategy": "priority",
                "rules": [
                    {
                        "match": {"model": "*"},
                        "channels": [
                            {"name": "channel_a", "weight": 1},
                            {"name": "channel_b", "weight": 1}
                        ],
                        "strategy": "priority"
                    }
                ]
            },
             {
                "name": "model-match-router",
                "vkey": "m-key",
                "channels": [],
                "strategy": "priority",
                "rules": [
                    {
                        "match": {"model": "gpt-4"},
                        "channels": [{"name": "channel_a", "weight": 1}],
                        "strategy": "priority"
                    },
                    {
                        "match": {"model": "*"},
                        "channels": [{"name": "channel_b", "weight": 1}],
                        "strategy": "priority"
                    }
                ]
            }
        ],
        "metrics": {
            "enabled": False,
            "listen": "127.0.0.1:9091",
            "path": "/metrics"
        },
        "hot_reload": {
            "config_path": "tests/e2e/temp_router_config.json",
            "watch": False
        }
    }

    config_path = Path("tests/e2e/temp_router_config.json").resolve()
    with open(config_path, "w") as f:
        json.dump(config, f, indent=2)
        
    # Start Apex
        proc = subprocess.Popen(
            [str(APEX_BIN), "gateway", "start", str(config_path)],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True
        )
    
    time.sleep(2) # Wait for Apex to start
    
    # Check if Apex is running
    if proc.poll() is not None:
        stdout, stderr = proc.communicate()
        print("Apex failed to start:")
        print("STDOUT:", stdout)
        print("STDERR:", stderr)
        pytest.fail("Apex failed to start")

    yield "http://127.0.0.1:8080"
    
    proc.terminate()
    try:
        stdout, stderr = proc.communicate(timeout=2)
        print("Apex Output:")
        print(stdout)
        print(stderr)
    except:
        proc.kill()
    
    if config_path.exists():
        os.remove(config_path)

def send_request(base_url, vkey, model="test-model"):
    headers = {
        "Authorization": f"Bearer {vkey}",
        "Content-Type": "application/json"
    }
    data = {
        "model": model,
        "messages": [{"role": "user", "content": "Hello"}]
    }
    try:
        response = requests.post(f"{base_url}/v1/chat/completions", headers=headers, json=data, timeout=5)
        if response.status_code != 200:
            print(f"Request failed: {response.status_code} {response.text}")
        return response.json() if response.status_code == 200 else None
    except Exception as e:
        print(f"Request failed: {e}")
        return None

def test_priority_strategy(apex_server):
    print("Testing Priority Strategy...")
    # Priority should always pick the first channel (A)
    for _ in range(5):
        resp = send_request(apex_server, "p-key")
        assert resp is not None
        content = resp["choices"][0]["message"]["content"]
        assert "Response from A" in content

def test_round_robin_strategy(apex_server):
    print("Testing Round Robin Strategy...")
    # Round Robin should pick both A and B roughly equally
    responses = []
    for _ in range(20):
        resp = send_request(apex_server, "rr-key")
        if resp:
            content = resp["choices"][0]["message"]["content"]
            responses.append(content)
    
    a_count = sum(1 for r in responses if "Response from A" in r)
    b_count = sum(1 for r in responses if "Response from B" in r)
    
    print(f"Round Robin counts: A={a_count}, B={b_count}")
    assert a_count > 0
    assert b_count > 0

def test_model_matching(apex_server):
    print("Testing Model Matching...")
    
    # 1. Match gpt-4 -> Channel A
    resp = send_request(apex_server, "m-key", model="gpt-4")
    assert resp is not None
    assert "Response from A" in resp["choices"][0]["message"]["content"]
    
    # 2. Match anything else -> Channel B (fallback rule)
    resp = send_request(apex_server, "m-key", model="gpt-3.5")
    assert resp is not None
    assert "Response from B" in resp["choices"][0]["message"]["content"]

if __name__ == "__main__":
    # If run directly, we need to manually invoke fixtures setup/teardown logic 
    # or just use pytest.
    # But for simplicity in "run_command", let's assume the user runs this with pytest.
    pass
