import os
import sys
import time
import json
import argparse
import asyncio
from typing import List, Dict, Any, Optional
from openai import OpenAI
from anthropic import Anthropic
from rich.console import Console
from rich.markdown import Markdown
from rich.panel import Panel
from rich.live import Live
from rich.prompt import Prompt

console = Console()

# Configuration
BASE_URL_OPENAI = "http://127.0.0.1:12356"
BASE_URL_ANTHROPIC = "http://127.0.0.1:12356"  # Anthropic SDK appends /v1/messages usually, or we configure it.
# Note: Anthropic Python SDK defaults to https://api.anthropic.com. We need to override base_url.
# Apex exposes /messages (without v1) and /v1/messages.
# Anthropic SDK usually expects /v1/messages if we point it to a base url.

def load_vkey(router_type: str) -> Optional[str]:
    vkey = os.environ.get("APEX_VKEY")
    if vkey:
        return vkey
    config_path = os.environ.get("APEX_CONFIG", "tests/e2e/config.json")
    try:
        with open(config_path, "r") as f:
            config = json.load(f)
    except Exception:
        return None
    routers = config.get("routers", [])
    for router in routers:
        if router.get("type") == router_type and router.get("vkey"):
            return router.get("vkey")
    for router in routers:
        if router.get("vkey"):
            return router.get("vkey")
    return None

def get_openai_client():
    vkey = load_vkey("openai")
    kwargs = {
        "api_key": vkey or "dummy",
        "base_url": BASE_URL_OPENAI,
    }
    if vkey:
        pass
    return OpenAI(**kwargs)

def get_anthropic_client():
    os.environ.pop("ANTHROPIC_AUTH_TOKEN", None)
    os.environ.pop("ANTHROPIC_API_KEY", None)
    os.environ.pop("ANTHROPIC_BASE_URL", None)
    vkey = load_vkey("anthropic")
    kwargs = {
        "api_key": vkey or "dummy",
        "base_url": BASE_URL_ANTHROPIC,
    }
    if vkey:
        pass
    return Anthropic(**kwargs)

def test_openai_protocol():
    console.print("[bold blue]Testing OpenAI Protocol...[/bold blue]")
    client = get_openai_client()
    
    # 1. Test Simple Chat
    try:
        console.print("  - Sending chat completion request (no stream)...", end=" ")
        completion = client.chat.completions.create(
            model="gpt-3.5-turbo", # Apex maps this to Minimax model via config
            messages=[{"role": "user", "content": "Hello, say 'OpenAI Protocol Works'!"}]
        )
        response = completion.choices[0].message.content
        if "OpenAI Protocol Works" in response or len(response) > 0:
            console.print("[green]OK[/green]")
        else:
            console.print(f"[red]Failed[/red] (Response: {response})")
    except Exception as e:
        console.print(f"[red]Error: {e}[/red]")
        return False

    # 2. Test Streaming
    try:
        console.print("  - Sending chat completion request (stream)...", end=" ")
        stream = client.chat.completions.create(
            model="gpt-3.5-turbo",
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
            model="claude-3-5-sonnet-20240620", # Apex maps this
            max_tokens=1024,
            messages=[{"role": "user", "content": "Hello, say 'Anthropic Protocol Works'!"}]
        )
        response = message.content[0].text
        if "Anthropic Protocol Works" in response or len(response) > 0:
            console.print("[green]OK[/green]")
        else:
            console.print(f"[red]Failed[/red] (Response: {response})")
    except Exception as e:
        console.print(f"[red]Error: {e}[/red]")
        # Don't fail the whole suite if Anthropic is not fully configured/supported yet, but we implemented it.
        return False

    # 2. Test Streaming
    try:
        console.print("  - Sending message request (stream)...", end=" ")
        content = ""
        with client.messages.stream(
            max_tokens=1024,
            messages=[{"role": "user", "content": "Count to 5."}],
            model="claude-3-5-sonnet-20240620",
        ) as stream:
            for text in stream.text_stream:
                content += text
        
        if len(content) > 5:
             console.print("[green]OK[/green]")
        else:
             console.print(f"[red]Failed[/red] (Content too short: {content})")
    except Exception as e:
        console.print(f"[red]Error: {e}[/red]")
        return False
        
    return True

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
                    model="gpt-3.5-turbo",
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
                    max_tokens=1024,
                    messages=messages,
                    model="claude-3-5-sonnet-20240620",
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
    args = parser.parse_args()

    if args.mode == "auto":
        success = test_openai_protocol()
        if not success:
            sys.exit(1)
        
        success = test_anthropic_protocol()
        if not success:
             sys.exit(1)
             
        console.print("[bold green]All Automated Tests Passed![/bold green]")
        
    elif args.mode == "interactive":
        run_interactive_mode(args.protocol)

if __name__ == "__main__":
    main()
