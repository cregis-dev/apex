#!/usr/bin/env python3
import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any


DEFAULT_PROMPT = (
    "Research what Google Gemini Deep Research Agent is. "
    "Keep the answer short and include only the most important points."
)


def env_bool(name: str, default: bool = False) -> bool:
    raw = os.environ.get(name)
    if raw is None:
        return default
    return raw.lower() in {"1", "true", "yes"}


def request_json(
    method: str,
    url: str,
    *,
    api_key: str,
    payload: dict[str, Any] | None = None,
    timeout: int = 180,
) -> tuple[int, dict[str, str], dict[str, Any] | list[Any] | None, bytes]:
    headers = {"Authorization": f"Bearer {api_key}"}
    data = None
    if payload is not None:
        data = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        headers["Content-Type"] = "application/json"

    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            body = resp.read()
            return resp.status, dict(resp.headers), decode_json(body), body
    except urllib.error.HTTPError as err:
        body = err.read()
        return err.code, dict(err.headers), decode_json(body), body


def decode_json(body: bytes) -> dict[str, Any] | list[Any] | None:
    if not body:
        return None
    try:
        value = json.loads(body.decode("utf-8"))
    except json.JSONDecodeError:
        return None
    if isinstance(value, (dict, list)):
        return value
    return None


def compact(value: Any, limit: int = 1600) -> str:
    text = json.dumps(value, ensure_ascii=False, indent=2) if value is not None else ""
    return text if len(text) <= limit else text[:limit] + "\n...<truncated>"


def body_preview(body: bytes, limit: int = 1600) -> str:
    text = body.decode("utf-8", errors="replace")
    return text if len(text) <= limit else text[:limit] + "\n...<truncated>"


def error_message(decoded: dict[str, Any] | list[Any] | None, body: bytes) -> str:
    if isinstance(decoded, dict):
        error = decoded.get("error")
        if isinstance(error, dict) and isinstance(error.get("message"), str):
            return error["message"]
        return compact(decoded)
    return body_preview(body)


def extract_interaction_id(value: dict[str, Any] | list[Any] | None) -> str | None:
    if not isinstance(value, dict):
        return None
    for key in ("id", "name"):
        candidate = value.get(key)
        if isinstance(candidate, str) and candidate:
            return candidate
    interaction = value.get("interaction")
    if isinstance(interaction, dict):
        return extract_interaction_id(interaction)
    return None


def interaction_status(value: dict[str, Any] | list[Any] | None) -> str:
    if not isinstance(value, dict):
        return "unknown"
    for key in ("status", "state"):
        candidate = value.get(key)
        if isinstance(candidate, str) and candidate:
            return candidate
    metadata = value.get("metadata")
    if isinstance(metadata, dict):
        for key in ("status", "state"):
            candidate = metadata.get(key)
            if isinstance(candidate, str) and candidate:
                return candidate
    return "unknown"


def is_terminal_status(status: str) -> bool:
    return status.lower() in {
        "completed",
        "complete",
        "done",
        "succeeded",
        "success",
        "failed",
        "cancelled",
        "canceled",
    }


def write_json(path: Path | None, value: Any) -> None:
    if path is None:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def build_payload(args: argparse.Namespace) -> dict[str, Any]:
    payload: dict[str, Any] = {
        "input": args.prompt,
        "agent": args.agent,
        "background": True,
        "store": args.store,
    }
    if args.file_search_store:
        payload["tools"] = [
            {
                "type": "file_search",
                "file_search_store_names": [args.file_search_store],
            }
        ]
    return payload


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Create and poll a Gemini Deep Research interaction through Apex."
    )
    parser.add_argument(
        "--base-url",
        default=os.environ.get("APEX_BASE_URL", "http://127.0.0.1:12357"),
        help="Apex base URL. Defaults to APEX_BASE_URL or http://127.0.0.1:12357.",
    )
    parser.add_argument(
        "--team-key",
        default=os.environ.get("APEX_TEAM_KEY", "sk-gemini-native-team"),
        help="Apex team API key. Defaults to APEX_TEAM_KEY.",
    )
    parser.add_argument(
        "--agent",
        default=os.environ.get(
            "APEX_GEMINI_NATIVE_DEEP_RESEARCH_AGENT",
            "deep-research-pro-preview-12-2025",
        ),
        help="Deep Research agent id.",
    )
    parser.add_argument(
        "--prompt",
        default=os.environ.get("APEX_GEMINI_INTERACTIONS_PROMPT", DEFAULT_PROMPT),
        help="Research prompt.",
    )
    parser.add_argument(
        "--interaction-id",
        default=os.environ.get("APEX_GEMINI_INTERACTIONS_ID", ""),
        help="Existing interaction id/name to poll. When set, the client skips POST creation.",
    )
    parser.add_argument(
        "--file-search-store",
        default=os.environ.get("APEX_GEMINI_NATIVE_FILE_SEARCH_STORE_NAME", ""),
        help="Optional File Search Store name to attach.",
    )
    parser.add_argument(
        "--polls",
        type=int,
        default=int(os.environ.get("APEX_GEMINI_INTERACTIONS_POLLS", "1")),
        help="Number of GET polls after creation. Use a larger value to wait for completion.",
    )
    parser.add_argument(
        "--poll-interval",
        type=float,
        default=float(os.environ.get("APEX_GEMINI_INTERACTIONS_POLL_INTERVAL", "5")),
        help="Seconds between polls.",
    )
    parser.add_argument(
        "--timeout",
        type=int,
        default=int(os.environ.get("APEX_GEMINI_INTERACTIONS_TIMEOUT", "180")),
        help="HTTP timeout in seconds.",
    )
    parser.add_argument(
        "--store",
        action=argparse.BooleanOptionalAction,
        default=env_bool("APEX_GEMINI_INTERACTIONS_STORE", True),
        help="Pass store true/false to the Interactions API.",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(os.environ["APEX_GEMINI_INTERACTIONS_OUTPUT"])
        if os.environ.get("APEX_GEMINI_INTERACTIONS_OUTPUT")
        else None,
        help="Optional path to write the last JSON response.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    base_url = args.base_url.rstrip("/")
    if args.interaction_id:
        interaction_id = args.interaction_id
        decoded: dict[str, Any] | list[Any] | None = None
        print(f"[interactions-client] polling existing: {interaction_id}")
    else:
        create_url = f"{base_url}/gemini/v1beta/interactions"
        payload = build_payload(args)

        print(f"[interactions-client] POST {create_url}")
        print(f"[interactions-client] agent={args.agent}")
        status, headers, decoded, raw = request_json(
            "POST",
            create_url,
            api_key=args.team_key,
            payload=payload,
            timeout=args.timeout,
        )
        content_type = headers.get("content-type", headers.get("Content-Type", ""))
        if not (200 <= status < 300):
            print(
                f"[interactions-client] create FAILED: HTTP {status}, content-type={content_type}",
                file=sys.stderr,
            )
            print(error_message(decoded, raw), file=sys.stderr)
            return 1

        interaction_id = extract_interaction_id(decoded)
        if not interaction_id:
            print("[interactions-client] create returned no id/name", file=sys.stderr)
            print(compact(decoded), file=sys.stderr)
            return 1

        print(f"[interactions-client] created: {interaction_id}")
        print(f"[interactions-client] create status: {interaction_status(decoded)}")

    last = decoded
    poll_id = interaction_id.removeprefix("interactions/")
    poll_path_id = urllib.parse.quote(poll_id, safe="")
    poll_url = f"{base_url}/gemini/v1beta/interactions/{poll_path_id}"

    for index in range(max(args.polls, 0)):
        if index > 0:
            time.sleep(args.poll_interval)
        print(f"[interactions-client] GET {poll_url} ({index + 1}/{args.polls})")
        status, headers, decoded, raw = request_json(
            "GET",
            poll_url,
            api_key=args.team_key,
            timeout=args.timeout,
        )
        content_type = headers.get("content-type", headers.get("Content-Type", ""))
        if not (200 <= status < 300):
            print(
                f"[interactions-client] poll FAILED: HTTP {status}, content-type={content_type}",
                file=sys.stderr,
            )
            print(error_message(decoded, raw), file=sys.stderr)
            return 1
        last = decoded
        status_text = interaction_status(decoded)
        print(f"[interactions-client] poll status: {status_text}")
        if is_terminal_status(status_text):
            break

    write_json(args.output, last)
    if args.output is not None:
        print(f"[interactions-client] wrote response: {args.output}")
    print("[interactions-client] last response preview:")
    print(compact(last))
    return 0


if __name__ == "__main__":
    sys.exit(main())
