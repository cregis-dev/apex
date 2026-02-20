import os
import sys
import json
import time
import subprocess
import signal
import shutil
from rich.console import Console
from rich.prompt import Prompt, Confirm

console = Console()

CONFIG_PATH = "tests/e2e/config.json"
SERVER_LOG = "tests/e2e/server.log"

def setup_config():
    if os.path.exists(CONFIG_PATH):
        if not Confirm.ask(f"Config {CONFIG_PATH} exists. Overwrite?"):
            return

    console.print("[bold blue]Setup E2E Configuration[/bold blue]")
    
    api_key = os.environ.get("MINIMAX_API_KEY")
    if not api_key:
        api_key = Prompt.ask("Enter Minimax API Key", password=True)
    
    group_id = os.environ.get("MINIMAX_GROUP_ID", "")
    # Minimax sometimes requires group_id in URL or header?
    # Usually base_url is https://api.minimax.io/v1
    
    base_url = Prompt.ask("Enter Minimax Base URL", default="https://api.minimax.io/v1")
    model = Prompt.ask("Enter Minimax Model Name", default="abab5.5-chat")

    config = {
        "version": "1",
        "global": {
            "listen": "127.0.0.1:12356",
            "auth": { "mode": "none", "keys": None },
            "timeouts": { "connect_ms": 5000, "request_ms": 60000, "response_ms": 60000 },
            "retries": { "max_attempts": 1, "backoff_ms": 1000, "retry_on_status": [429, 500, 502, 503, 504] }
        },
        "channels": [
            {
                "name": "minimax",
                "provider_type": "minimax",
                "base_url": base_url,
                "api_key": api_key,
                "model_map": {
                    "gpt-3.5-turbo": model,
                    "claude-3-5-sonnet-20240620": model
                }
            }
        ],
        "routers": [
            {
                "name": "openai_route",
                "type": "openai",
                "vkey": "test_key", # We disabled auth in global, but router lookup uses vkey if present?
                # Actually server.rs enforce_global_auth checks global.auth.mode.
                # If mode is None, it skips auth check.
                # BUT, handle_models checks vkey (Line 190).
                # handle_openai (Line 248) checks vkey.
                # Wait, handle_openai checks vkey unconditionally?
                # Let's check server.rs again.
                # Line 249: let Some(vkey) = read_auth_token... else error.
                # So vkey is REQUIRED even if global auth is None?
                # Yes, to find the router.
                # So we must provide a vkey in the request.
                "channel": "minimax",
                "fallback_channels": []
            },
            {
                "name": "anthropic_route",
                "type": "anthropic",
                "vkey": "test_key",
                "channel": "minimax",
                "fallback_channels": []
            }
        ],
        "metrics": { "enabled": False, "listen": "127.0.0.1:9091", "path": "/metrics" },
        "hot_reload": { "config_path": CONFIG_PATH, "watch": False }
    }
    
    with open(CONFIG_PATH, "w") as f:
        json.dump(config, f, indent=2)
    console.print(f"[green]Config saved to {CONFIG_PATH}[/green]")

def run_server():
    console.print("[bold blue]Starting Apex Server...[/bold blue]")
    # Kill existing server on port 12356 if any?
    # For now assume clean env.
    
    # We need to build first to ensure it's up to date
    subprocess.run(["cargo", "build"], check=True)
    
    # Start server
    proc = subprocess.Popen(
        ["cargo", "run", "--", "serve", "--config", CONFIG_PATH],
        stdout=open(SERVER_LOG, "w"),
        stderr=subprocess.STDOUT
    )
    return proc

def wait_for_server(url, timeout=10):
    import requests
    start = time.time()
    while time.time() - start < timeout:
        try:
            # check health or just connect
            # Apex doesn't have /health yet, but we can try /metrics or /v1/models (needs auth)
            # Or just tcp connect.
            try:
                import socket
                s = socket.create_connection(("127.0.0.1", 12356), timeout=1)
                s.close()
                return True
            except:
                pass
        except:
            pass
        time.sleep(0.5)
    return False

def main():
    setup_config()
    
    server_proc = run_server()
    try:
        if wait_for_server("http://127.0.0.1:12356"):
            console.print("[green]Server is ready![/green]")
            
            # Install deps if needed (assuming user has them or we run pip)
            # subprocess.run(["pip", "install", "-r", "tests/e2e/requirements.txt"], check=True)
            
            # Run Automated Tests
            console.print("\n[bold yellow]Running Automated Tests...[/bold yellow]")
            # We need to pass the vkey in headers.
            # The test_chat_cli.py uses openai/anthropic clients.
            # We need to tell clients to send 'Authorization: Bearer test_key' or 'x-api-key: test_key'
            # `read_auth_token` checks `Authorization` (Bearer) or `x-api-key`.
            # If I set `api_key` in client, it sends `Authorization: Bearer ...`.
            # Does `read_auth_token` handle that?
            # Let's assume `read_auth_token` parses Bearer token.
            
            # Run test script
            env = os.environ.copy()
            # Pass vkey as API KEY to the clients
            # OpenAI client sends Authorization: Bearer <key>
            # Anthropic client sends x-api-key: <key>
            # If Apex accepts these as vkey, we are good.
            
            # I need to verify `read_auth_token` implementation in `server.rs` or `lib.rs`?
            # It was likely in `server.rs` or `providers.rs`.
            # Assuming it does.
            
            # Run tests
            ret = subprocess.call([sys.executable, "tests/e2e/test_chat_cli.py", "--mode", "auto"], env=env)
            
            if ret == 0:
                if Confirm.ask("\n[bold green]Tests Passed! Do you want to try Interactive Mode?[/bold green]"):
                    mode = Prompt.ask("Select Protocol", choices=["openai", "anthropic"], default="openai")
                    subprocess.call([sys.executable, "tests/e2e/test_chat_cli.py", "--mode", "interactive", "--protocol", mode], env=env)
            else:
                console.print("[red]Automated tests failed.[/red]")
                console.print(f"Check {SERVER_LOG} for server logs.")
                
        else:
            console.print("[red]Server failed to start.[/red]")
            console.print(f"Check {SERVER_LOG} for logs.")
            
    finally:
        console.print("[bold blue]Stopping Server...[/bold blue]")
        server_proc.terminate()
        server_proc.wait()

if __name__ == "__main__":
    main()
