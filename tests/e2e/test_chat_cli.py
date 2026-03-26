import os
import sys
import json
import argparse
from typing import Callable, List, Dict, Any, Optional
from openai import OpenAI
from anthropic import Anthropic
from rich.console import Console
from rich.markdown import Markdown
from rich.live import Live
from rich.prompt import Prompt

console = Console()

# Configuration
BASE_URL_OPENAI = os.environ.get("APEX_BASE_URL", "http://127.0.0.1:12356")
BASE_URL_ANTHROPIC = os.environ.get("APEX_BASE_URL", "http://127.0.0.1:12356")
TEST_MODEL = os.environ.get("APEX_TEST_MODEL", "apex-test-chat")
DEFAULT_PROTOCOLS = ["openai", "anthropic"]

def load_api_key(router_type: str) -> Optional[str]:
    api_key = os.environ.get("APEX_TEAM_KEY") or os.environ.get("APEX_VKEY")
    if api_key:
        return api_key
    config_path = os.environ.get("APEX_CONFIG", "tests/e2e/manual_config.json")
    try:
        with open(config_path, "r") as f:
            config = json.load(f)
    except Exception:
        return None

    teams = config.get("teams", [])
    if teams:
        team_key = teams[0].get("api_key")
        if team_key:
            return team_key

    # Backward-compatible fallback for older E2E configs
    routers = config.get("routers", [])
    for router in routers:
        if router.get("type") == router_type and router.get("vkey"):
            return router.get("vkey")
    for router in routers:
        if router.get("vkey"):
            return router.get("vkey")
    return None

def load_config() -> Dict[str, Any]:
    config_path = os.environ.get("APEX_CONFIG", "tests/e2e/manual_config.json")
    try:
        with open(config_path, "r") as f:
            return json.load(f)
    except Exception:
        return {}

def configured_protocols() -> List[str]:
    config = load_config()
    protocols: List[str] = []
    openai_like = {
        "openai",
        "deepseek",
        "moonshot",
        "minimax",
        "ollama",
        "openrouter",
        "gemini",
        "jina",
        "zai",
    }
    anthropic_compatible = set(openai_like)
    anthropic_compatible.update({"anthropic", "customdual"})

    for channel in config.get("channels", []):
        provider_type = channel.get("provider_type")
        if provider_type in openai_like and "openai" not in protocols:
            protocols.append("openai")
        if provider_type in anthropic_compatible and "anthropic" not in protocols:
            protocols.append("anthropic")

    if not protocols:
        protocols.append("openai")
    return protocols

def parse_protocols(raw: Optional[str]) -> List[str]:
    if not raw:
        return DEFAULT_PROTOCOLS.copy()
    protocols = [item.strip() for item in raw.split(",") if item.strip()]
    return protocols or DEFAULT_PROTOCOLS.copy()

def extract_anthropic_content(blocks: List[Any]) -> str:
    parts: List[str] = []
    for block in blocks:
        text = getattr(block, "text", None)
        if text:
            parts.append(text)
            continue
        thinking = getattr(block, "thinking", None)
        if thinking:
            parts.append(thinking)
    return "\n".join(parts).strip()

def get_openai_client():
    api_key = load_api_key("openai")
    kwargs = {
        "api_key": api_key or "dummy",
        "base_url": BASE_URL_OPENAI,
    }
    if api_key:
        pass
    return OpenAI(**kwargs)

def get_anthropic_client():
    os.environ.pop("ANTHROPIC_AUTH_TOKEN", None)
    os.environ.pop("ANTHROPIC_API_KEY", None)
    os.environ.pop("ANTHROPIC_BASE_URL", None)
    api_key = load_api_key("anthropic")
    kwargs = {
        "api_key": api_key or "dummy",
        "base_url": BASE_URL_ANTHROPIC,
    }
    if api_key:
        pass
    return Anthropic(**kwargs)

def test_openai_protocol():
    console.print("[bold blue]Testing OpenAI Protocol...[/bold blue]")
    client = get_openai_client()
    
    # 1. Test Simple Chat
    try:
        console.print("  - Sending chat completion request (no stream)...", end=" ")
        completion = client.chat.completions.create(
            model=TEST_MODEL,
            messages=[{"role": "user", "content": "Hello, say 'OpenAI Protocol Works'!"}]
        )
        response = completion.choices[0].message.content
        if "OpenAI Protocol Works" in response or len(response) > 0:
            console.print("[green]OK[/green]")
        else:
            console.print(f"[red]Failed[/red] (Response: {response})")
            return False
    except Exception as e:
        console.print(f"[red]Error: {e}[/red]")
        return False

    # 2. Test Streaming
    try:
        console.print("  - Sending chat completion request (stream)...", end=" ")
        stream = client.chat.completions.create(
            model=TEST_MODEL,
            messages=[{"role": "user", "content": "Count to 5."}],
            stream=True
        )
        content = ""
        for chunk in stream:
            if chunk.choices[0].delta.content:
                content += chunk.choices[0].delta.content
        
        if len(content) > 5:
             console.print("[green]OK[/green]")
        else:
             console.print(f"[red]Failed[/red] (Content too short: {content})")
    except Exception as e:
        console.print(f"[red]Error: {e}[/red]")
        return False
        
    return True

def test_anthropic_protocol():
    console.print("[bold blue]Testing Anthropic Protocol...[/bold blue]")
    client = get_anthropic_client()
    
    # 1. Test Messages
    try:
        console.print("  - Sending message request (no stream)...", end=" ")
        message = client.messages.create(
            model=TEST_MODEL,
            max_tokens=16,
            messages=[{"role": "user", "content": "Hello"}]
        )
        response = extract_anthropic_content(message.content)
        if len(response) > 0:
            console.print("[green]OK[/green]")
        else:
            console.print(f"[red]Failed[/red] (Response: {response})")
            return False
    except Exception as e:
        console.print(f"[red]Error: {e}[/red]")
        # Don't fail the whole suite if Anthropic is not fully configured/supported yet, but we implemented it.
        return False

    # 2. Test Streaming
    try:
        console.print("  - Sending message request (stream)...", end=" ")
        content = ""
        with client.messages.stream(
            max_tokens=64,
            messages=[{"role": "user", "content": "Reply with exactly OK and nothing else."}],
            model=TEST_MODEL,
        ) as stream:
            for text in stream.text_stream:
                content += text
            if len(content) <= 5:
                final_message = stream.get_final_message()
                content = extract_anthropic_content(final_message.content)
        
        if len(content) > 5:
             console.print("[green]OK[/green]")
        else:
             console.print(f"[red]Failed[/red] (Content too short: {content})")
             return False
    except Exception as e:
        console.print(f"[red]Error: {e}[/red]")
        return False

    # 3. Test Claude Code style tool history
    try:
        console.print("  - Sending Claude Code style tool history request...", end=" ")
        message = client.messages.create(
            model=TEST_MODEL,
            max_tokens=32,
            system=[{"type": "text", "text": "You are Claude Code."}],
            tools=[
                {
                    "name": "run_command",
                    "description": "Execute a shell command",
                    "input_schema": {
                        "type": "object",
                        "properties": {"cmd": {"type": "string"}},
                        "required": ["cmd"],
                    },
                }
            ],
            messages=[
                {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "I'll inspect the repo."},
                        {
                            "type": "tool_use",
                            "id": "toolu_123",
                            "name": "run_command",
                            "input": {"cmd": "pwd"},
                        },
                    ],
                },
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "tool_result",
                            "tool_use_id": "toolu_123",
                            "content": [{"type": "text", "text": "/workspace/apex"}],
                        },
                        {"type": "text", "text": "Continue and answer briefly."},
                    ],
                },
            ],
        )
        response = extract_anthropic_content(message.content)
        if len(response) > 0:
            console.print("[green]OK[/green]")
        else:
            console.print(f"[red]Failed[/red] (Response: {response})")
            return False
    except Exception as e:
        console.print(f"[red]Error: {e}[/red]")
        return False
        
    return True

def run_auto_mode(protocols: List[str]) -> int:
    configured = configured_protocols()
    console.print(f"[bold blue]Target Protocols:[/bold blue] {', '.join(protocols)}")
    console.print(f"[bold blue]Configured Protocols:[/bold blue] {', '.join(configured)}")

    protocol_runners: Dict[str, Callable[[], bool]] = {
        "openai": test_openai_protocol,
        "anthropic": test_anthropic_protocol,
    }

    for protocol in protocols:
        runner = protocol_runners.get(protocol)
        if runner is None:
            console.print(f"[red]Unsupported protocol: {protocol}[/red]")
            return 1
        if not runner():
            return 1

    console.print("[bold green]All Automated Tests Passed![/bold green]")
    return 0

def run_interactive_mode(protocol="openai"):
    console.print(f"[bold green]Starting Interactive Mode ({protocol})[/bold green]")
    console.print("Type 'exit' or 'quit' to stop.")
    
    messages = []
    
    while True:
        user_input = Prompt.ask("[bold yellow]User[/bold yellow]")
        if user_input.lower() in ["exit", "quit"]:
            break
            
        messages.append({"role": "user", "content": user_input})
        
        console.print("[bold cyan]Apex[/bold cyan]: ", end="")
        
        try:
            full_response = ""
            if protocol == "openai":
                client = get_openai_client()
                stream = client.chat.completions.create(
                    model=TEST_MODEL,
                    messages=messages,
                    stream=True
                )
                with Live(Markdown(""), refresh_per_second=10) as live:
                    for chunk in stream:
                        if chunk.choices[0].delta.content:
                            content = chunk.choices[0].delta.content
                            full_response += content
                            live.update(Markdown(full_response))
                console.print() # Newline
                messages.append({"role": "assistant", "content": full_response})
                
            elif protocol == "anthropic":
                client = get_anthropic_client()
                # Anthropic doesn't support persistent history in same way, we pass full history?
                # Anthropic SDK handles history if we manage the list.
                # But Anthropic API doesn't allow 'system' messages in the middle, and strict role alternating.
                # For simplicity, we just send user message or basic history.
                
                with client.messages.stream(
                    max_tokens=16,
                    messages=messages,
                    model=TEST_MODEL,
                ) as stream:
                     with Live(Markdown(""), refresh_per_second=10) as live:
                        for text in stream.text_stream:
                            full_response += text
                            live.update(Markdown(full_response))
                console.print()
                messages.append({"role": "assistant", "content": full_response})
                
        except Exception as e:
            console.print(f"[bold red]Error: {e}[/bold red]")

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=["auto", "interactive"], default="auto")
    parser.add_argument("--protocol", choices=["openai", "anthropic"], default="openai")
    parser.add_argument(
        "--protocols",
        default=os.environ.get("APEX_E2E_PROTOCOLS", ",".join(DEFAULT_PROTOCOLS)),
        help="Comma-separated protocol list for auto mode",
    )
    args = parser.parse_args()

    if args.mode == "auto":
        sys.exit(run_auto_mode(parse_protocols(args.protocols)))
        
    elif args.mode == "interactive":
        run_interactive_mode(args.protocol)

if __name__ == "__main__":
    main()
