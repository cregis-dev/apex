import os
import sys
import time
import socket
import subprocess
import json
from pathlib import Path

from rich.console import Console
from rich.prompt import Confirm, Prompt

console = Console()

ROOT_DIR = Path(__file__).resolve().parents[2]
ENV_FILE = Path(os.environ.get("APEX_ENV_FILE", ROOT_DIR / ".env.e2e"))
CONFIG_PATH = Path(
    os.environ.get("APEX_CONFIG", ROOT_DIR / "tests/e2e/generated.e2e.config.json")
)
SERVER_LOG = Path(
    os.environ.get("APEX_SERVER_LOG", ROOT_DIR / "tests/e2e/server.log")
)


def generate_config():
    if not ENV_FILE.exists():
        raise FileNotFoundError(
            f"{ENV_FILE} does not exist. Copy .env.e2e.example to .env.e2e first."
        )

    console.print("[bold blue]Generating E2E Config...[/bold blue]")
    subprocess.run(
        [
            "cargo",
            "run",
            "--bin",
            "apex-e2e-config",
            "--",
            "--env-file",
            str(ENV_FILE),
            "--output",
            str(CONFIG_PATH),
        ],
        check=True,
        cwd=ROOT_DIR,
    )

def load_generated_runtime():
    with open(CONFIG_PATH, "r") as f:
        config = json.load(f)

    listen = config.get("global", {}).get("listen", "127.0.0.1:12356")
    if ":" not in listen:
        raise ValueError(f"Unsupported listen address in config: {listen}")

    host, port_text = listen.rsplit(":", 1)
    teams = config.get("teams", [])
    team_key = teams[0]["api_key"] if teams else None
    allowed_models = teams[0].get("policy", {}).get("allowed_models") if teams else None
    test_model = allowed_models[0] if allowed_models else "apex-test-chat"
    return host, int(port_text), team_key, test_model


def run_server():
    console.print("[bold blue]Starting Apex Server...[/bold blue]")
    SERVER_LOG.parent.mkdir(parents=True, exist_ok=True)
    log_file = open(SERVER_LOG, "w")
    return subprocess.Popen(
        [
            "cargo",
            "run",
            "--bin",
            "apex",
            "--",
            "--config",
            str(CONFIG_PATH),
            "gateway",
            "start",
        ],
        cwd=ROOT_DIR,
        stdout=log_file,
        stderr=subprocess.STDOUT,
    )


def wait_for_server(host: str, port: int, timeout: int = 10) -> bool:
    start = time.time()
    while time.time() - start < timeout:
        try:
            with socket.create_connection((host, port), timeout=1):
                return True
        except OSError:
            time.sleep(0.5)
    return False


def main():
    generate_config()
    host, port, team_key, test_model = load_generated_runtime()
    base_url = f"http://{host}:{port}"

    server_proc = run_server()
    try:
        if wait_for_server(host, port):
            console.print("[green]Server is ready![/green]")

            env = os.environ.copy()
            env["APEX_CONFIG"] = str(CONFIG_PATH)
            env.setdefault("APEX_BASE_URL", base_url)
            if team_key:
                env.setdefault("APEX_TEAM_KEY", team_key)
            env.setdefault("APEX_TEST_MODEL", test_model)

            console.print("\n[bold yellow]Running Automated Tests...[/bold yellow]")
            ret = subprocess.call(
                [sys.executable, "tests/e2e/test_chat_cli.py", "--mode", "auto"],
                env=env,
                cwd=ROOT_DIR,
            )

            if ret == 0:
                if sys.stdin.isatty() and Confirm.ask(
                    "\n[bold green]Tests Passed! Do you want to try Interactive Mode?[/bold green]"
                ):
                    mode = Prompt.ask(
                        "Select Protocol",
                        choices=["openai", "anthropic"],
                        default="openai",
                    )
                    subprocess.call(
                        [
                            sys.executable,
                            "tests/e2e/test_chat_cli.py",
                            "--mode",
                            "interactive",
                            "--protocol",
                            mode,
                        ],
                        env=env,
                        cwd=ROOT_DIR,
                    )
            else:
                console.print("[red]Automated tests failed.[/red]")
                console.print(f"Check {SERVER_LOG} for server logs.")
                sys.exit(ret)
        else:
            console.print("[red]Server failed to start.[/red]")
            console.print(f"Check {SERVER_LOG} for logs.")
            sys.exit(1)
    finally:
        console.print("[bold blue]Stopping Server...[/bold blue]")
        server_proc.terminate()
        try:
            server_proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            server_proc.kill()


if __name__ == "__main__":
    main()
