import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Dict, List


BASE_URL = os.environ.get("APEX_BASE_URL", "http://127.0.0.1:12357").rstrip("/")
TEAM_KEY = os.environ.get("APEX_TEAM_KEY", "sk-gemini-native-team")
ADMIN_KEY = os.environ.get("APEX_ADMIN_KEY", "sk-gemini-native-admin")
MODEL = os.environ.get("APEX_TEST_MODEL", "apex-gemini-native")
UPSTREAM_MODEL = os.environ.get("APEX_GEMINI_NATIVE_UPSTREAM_MODEL", "gemini-2.5-flash")
UPSTREAM_BASE_URL = os.environ.get(
    "APEX_GEMINI_NATIVE_BASE_URL", "https://generativelanguage.googleapis.com/v1beta"
).rstrip("/")
GEMINI_API_KEY = os.environ.get("GEMINI_API_KEY") or os.environ.get("APEX_GEMINI_API_KEY")
ENABLE_SEARCH = os.environ.get("APEX_GEMINI_NATIVE_ENABLE_SEARCH", "").lower() in {
    "1",
    "true",
    "yes",
}
ENABLE_TOOL_TESTS = os.environ.get("APEX_GEMINI_NATIVE_ENABLE_TOOL_TESTS", "").lower() in {
    "1",
    "true",
    "yes",
}
FILE_SEARCH_STORE_NAME = os.environ.get("APEX_GEMINI_NATIVE_FILE_SEARCH_STORE_NAME", "")
DEEP_RESEARCH_AGENT = os.environ.get(
    "APEX_GEMINI_NATIVE_DEEP_RESEARCH_AGENT",
    "deep-research-pro-preview-12-2025",
)


def is_enabled(name: str, *, default: bool = False) -> bool:
    raw = os.environ.get(f"APEX_GEMINI_NATIVE_ENABLE_{name.upper()}")
    if raw is None:
        return default
    return raw.lower() in {"1", "true", "yes"}


def request(
    method: str,
    path: str,
    payload: Dict[str, Any] | None = None,
    *,
    admin: bool = False,
    quiet_errors: bool = False,
) -> tuple[int, Dict[str, str], bytes]:
    headers = {
        "Authorization": f"Bearer {ADMIN_KEY if admin else TEAM_KEY}",
    }
    data = None
    if payload is not None:
        data = json.dumps(payload).encode("utf-8")
        headers["Content-Type"] = "application/json"

    req = urllib.request.Request(
        f"{BASE_URL}{path}",
        data=data,
        method=method,
        headers=headers,
    )
    try:
        with urllib.request.urlopen(req, timeout=120) as resp:
            return resp.status, dict(resp.headers), resp.read()
    except urllib.error.HTTPError as err:
        body = err.read()
        if not quiet_errors:
            print(body.decode("utf-8", errors="replace"), file=sys.stderr)
        return err.code, dict(err.headers), body


def request_absolute(
    method: str,
    url: str,
    payload: Dict[str, Any] | None = None,
    headers: Dict[str, str] | None = None,
) -> tuple[int, Dict[str, str], bytes]:
    merged_headers = dict(headers or {})
    data = None
    if payload is not None:
        data = json.dumps(payload).encode("utf-8")
        merged_headers["Content-Type"] = "application/json"

    req = urllib.request.Request(url, data=data, method=method, headers=merged_headers)
    try:
        with urllib.request.urlopen(req, timeout=120) as resp:
            return resp.status, dict(resp.headers), resp.read()
    except urllib.error.HTTPError as err:
        body = err.read()
        print(body.decode("utf-8", errors="replace"), file=sys.stderr)
        return err.code, dict(err.headers), body


def request_json(
    method: str,
    path: str,
    payload: Dict[str, Any] | None = None,
    *,
    admin: bool = False,
) -> Dict[str, Any]:
    status, _headers, body = request(method, path, payload, admin=admin)
    assert 200 <= status < 300, f"{method} {path} returned HTTP {status}"
    return json.loads(body.decode("utf-8"))


def request_json_status(
    method: str,
    path: str,
    payload: Dict[str, Any] | None = None,
    *,
    admin: bool = False,
) -> tuple[int, Dict[str, str], Dict[str, Any] | None, bytes]:
    status, headers, body = request(method, path, payload, admin=admin, quiet_errors=True)
    decoded = None
    if body:
        try:
            decoded = json.loads(body.decode("utf-8"))
        except json.JSONDecodeError:
            decoded = None
    return status, headers, decoded, body


def model_path(action: str = "") -> str:
    model = urllib.parse.quote(MODEL, safe="")
    return f"/gemini/v1beta/models/{model}{action}"


def extract_text(candidate: Dict[str, Any]) -> str:
    parts = candidate.get("content", {}).get("parts", [])
    return "".join(part.get("text", "") for part in parts if isinstance(part, dict))


def first_candidate(body: Dict[str, Any]) -> Dict[str, Any]:
    candidates = body.get("candidates") or []
    assert candidates, f"response returned no candidates: {body}"
    assert isinstance(candidates[0], dict), f"first candidate is not an object: {body}"
    return candidates[0]


def candidate_parts(candidate: Dict[str, Any]) -> List[Dict[str, Any]]:
    parts = candidate.get("content", {}).get("parts", [])
    return [part for part in parts if isinstance(part, dict)]


def contains_key(value: Any, keys: set[str]) -> bool:
    if isinstance(value, dict):
        return any(key in value for key in keys) or any(
            contains_key(child, keys) for child in value.values()
        )
    if isinstance(value, list):
        return any(contains_key(child, keys) for child in value)
    return False


def json_error_message(body_json: Dict[str, Any] | None, body: bytes) -> str:
    if isinstance(body_json, dict):
        error = body_json.get("error")
        if isinstance(error, dict):
            message = error.get("message")
            if isinstance(message, str):
                return message
        return json.dumps(body_json, ensure_ascii=False)[:1000]
    return body_preview(body)


def unsupported_tool_response(tool_name: str, status: int, message: str) -> bool:
    lowered = message.lower()
    markers = [
        "not supported",
        "does not support",
        "doesn't support",
        "not found for api version",
        "model is not found",
        "permission denied",
        "not enabled",
    ]
    if status in {400, 403, 404} and any(marker in lowered for marker in markers):
        print(
            f"{tool_name} tool skipped by upstream model/key "
            f"(HTTP {status}: {message[:180]})"
        )
        return True
    return False


def request_tool_json(tool_name: str, payload: Dict[str, Any]) -> Dict[str, Any] | None:
    status, _headers, body_json, raw_body = request_json_status(
        "POST", model_path(":generateContent"), payload
    )
    if 200 <= status < 300:
        assert isinstance(body_json, dict), f"{tool_name} returned non-JSON body: {raw_body!r}"
        return body_json

    message = json_error_message(body_json, raw_body)
    if unsupported_tool_response(tool_name, status, message):
        return None
    raise AssertionError(f"{tool_name} returned HTTP {status}: {message}")


def parse_sse(body: bytes) -> List[Dict[str, Any]]:
    events: List[Dict[str, Any]] = []
    text = body.decode("utf-8", errors="replace").strip()

    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line.startswith("data:"):
            continue
        data = line.removeprefix("data:").strip()
        if not data or data == "[DONE]":
            continue
        events.append(json.loads(data))
    if events:
        return events

    # Be tolerant of providers/proxies returning a JSON stream fallback instead
    # of literal SSE frames. The native route is still a pass-through path; this
    # keeps the smoke focused on semantics instead of one wire framing variant.
    if not text:
        return events
    try:
        decoded = json.loads(text)
    except json.JSONDecodeError:
        return events
    if isinstance(decoded, list):
        return [item for item in decoded if isinstance(item, dict)]
    if isinstance(decoded, dict):
        return [decoded]
    return events


def body_preview(body: bytes, limit: int = 1000) -> str:
    text = body.decode("utf-8", errors="replace").replace("\n", "\\n")
    return text[:limit]


def gemini_native_base_url() -> str:
    return UPSTREAM_BASE_URL.removesuffix("/openai")


def direct_stream_diagnostic(payload: Dict[str, Any]) -> str:
    if not GEMINI_API_KEY:
        return "direct Google diagnostic skipped: GEMINI_API_KEY not available to test process"

    upstream_model = urllib.parse.quote(UPSTREAM_MODEL, safe="")
    url = (
        f"{gemini_native_base_url()}/models/{upstream_model}:streamGenerateContent"
        "?alt=sse"
    )
    status, headers, body = request_absolute(
        "POST",
        url,
        payload,
        headers={"x-goog-api-key": GEMINI_API_KEY},
    )
    content_type = headers.get("content-type", headers.get("Content-Type", ""))
    events = parse_sse(body)
    return (
        f"direct_google_status={status}, direct_google_content_type={content_type}, "
        f"direct_google_events={len(events)}, direct_google_body={body_preview(body)}"
    )


def test_get_model() -> None:
    body = request_json("GET", model_path())
    name = body.get("name", "")
    assert name, f"model get returned no name: {body}"
    assert UPSTREAM_MODEL in name or MODEL in name, f"unexpected model name: {name}"
    print(f"GET model OK: {name}")


def test_generate_content() -> None:
    payload: Dict[str, Any] = {
        "contents": [
            {
                "role": "user",
                "parts": [{"text": "Reply with a short greeting in one sentence."}],
            }
        ],
        "generationConfig": {
            "temperature": 0,
            "maxOutputTokens": 64,
        },
    }
    body = request_json("POST", model_path(":generateContent"), payload)
    candidates = body.get("candidates") or []
    assert candidates, f"generateContent returned no candidates: {body}"
    text = extract_text(candidates[0])
    assert text.strip(), f"generateContent returned empty text: {body}"
    usage = body.get("usageMetadata") or {}
    assert usage.get("promptTokenCount") is not None, f"missing usageMetadata: {body}"
    print(f"generateContent OK: {text.strip()[:80]}")


def test_stream_generate_content() -> None:
    payload = {
        "contents": [
            {
                "role": "user",
                "parts": [{"text": "Write a story about a magic backpack in six paragraphs."}],
            }
        ],
        "generationConfig": {
            "temperature": 0,
            "maxOutputTokens": 1024,
        },
    }
    status, headers, body = request(
        "POST",
        f"{model_path(':streamGenerateContent')}?alt=sse",
        payload,
    )
    assert status == 200, f"streamGenerateContent returned HTTP {status}"
    content_type = headers.get("content-type", headers.get("Content-Type", ""))
    events = parse_sse(body)
    assert events, (
        "streamGenerateContent returned no parseable events "
        f"(content-type={content_type}, body={body_preview(body)}, "
        f"{direct_stream_diagnostic(payload)})"
    )
    assert any(event.get("candidates") for event in events), (
        f"no candidate events (content-type={content_type}, events={events}, "
        f"body={body_preview(body)})"
    )
    print(f"streamGenerateContent OK: {len(events)} event(s), content-type={content_type}")


def test_optional_google_search_tool() -> None:
    if not (ENABLE_SEARCH or ENABLE_TOOL_TESTS):
        print(
            "google_search tool test skipped "
            "(set APEX_GEMINI_NATIVE_ENABLE_SEARCH=1 or APEX_GEMINI_NATIVE_ENABLE_TOOL_TESTS=1)"
        )
        return

    payload = {
        "contents": [
            {
                "role": "user",
                "parts": [{"text": "Use Google Search and answer: what is Google's homepage URL?"}],
            }
        ],
        "tools": [{"google_search": {}}],
        "generationConfig": {
            "temperature": 0,
            "maxOutputTokens": 128,
        },
    }
    body = request_tool_json("google_search", payload)
    if body is None:
        return
    first_candidate(body)
    print("google_search tool pass-through OK")


def test_optional_code_execution_tool() -> None:
    if not is_enabled("code_execution", default=ENABLE_TOOL_TESTS):
        print("code_execution tool test skipped (set APEX_GEMINI_NATIVE_ENABLE_TOOL_TESTS=1)")
        return

    payload = {
        "contents": [
            {
                "role": "user",
                "parts": [
                    {
                        "text": (
                            "Use Python code execution to calculate the sum of squares from "
                            "1 through 10. Return the final numeric result."
                        )
                    }
                ],
            }
        ],
        "tools": [{"code_execution": {}}],
        "generationConfig": {
            "temperature": 0,
            "maxOutputTokens": 1024,
        },
    }
    body = request_tool_json("code_execution", payload)
    if body is None:
        return
    candidate = first_candidate(body)
    text = extract_text(candidate)
    assert "385" in text, f"code_execution response did not include expected result: {body}"
    assert contains_key(candidate_parts(candidate), {"executableCode", "codeExecutionResult"}), (
        f"code_execution response did not include execution parts: {body}"
    )
    print("code_execution tool pass-through OK")


def test_optional_url_context_tool() -> None:
    if not is_enabled("url_context", default=ENABLE_TOOL_TESTS):
        print("url_context tool test skipped (set APEX_GEMINI_NATIVE_ENABLE_TOOL_TESTS=1)")
        return

    payload = {
        "contents": [
            {
                "role": "user",
                "parts": [
                    {
                        "text": (
                            "Use URL context to read https://ai.google.dev/gemini-api/docs/tools "
                            "and name two built-in tools from that page."
                        )
                    }
                ],
            }
        ],
        "tools": [{"url_context": {}}],
        "generationConfig": {
            "temperature": 0,
            "maxOutputTokens": 512,
        },
    }
    body = request_tool_json("url_context", payload)
    if body is None:
        return
    candidate = first_candidate(body)
    text = extract_text(candidate).lower()
    assert "search" in text or "code" in text or "url" in text, (
        f"url_context response did not mention expected tool names: {body}"
    )
    assert contains_key(candidate, {"urlContextMetadata", "url_context_metadata"}), (
        f"url_context response missing URL metadata: {body}"
    )
    print("url_context tool pass-through OK")


def test_optional_google_maps_tool() -> None:
    if not is_enabled("google_maps", default=ENABLE_TOOL_TESTS):
        print("googleMaps tool test skipped (set APEX_GEMINI_NATIVE_ENABLE_TOOL_TESTS=1)")
        return

    payload = {
        "contents": [
            {
                "role": "user",
                "parts": [
                    {
                        "text": (
                            "Using Google Maps, name one cafe or restaurant near Times Square "
                            "in New York City."
                        )
                    }
                ],
            }
        ],
        "tools": [{"googleMaps": {}}],
        "toolConfig": {
            "retrievalConfig": {
                "latLng": {
                    "latitude": 40.758896,
                    "longitude": -73.985130,
                }
            }
        },
        "generationConfig": {
            "temperature": 0,
            "maxOutputTokens": 512,
        },
    }
    body = request_tool_json("googleMaps", payload)
    if body is None:
        return
    candidate = first_candidate(body)
    assert extract_text(candidate).strip(), f"googleMaps returned empty text: {body}"
    assert contains_key(candidate, {"groundingMetadata", "grounding_metadata"}), (
        f"googleMaps response missing grounding metadata: {body}"
    )
    print("googleMaps tool pass-through OK")


def test_optional_file_search_routes() -> None:
    if not is_enabled("file_search", default=ENABLE_TOOL_TESTS):
        print("file_search route test skipped (set APEX_GEMINI_NATIVE_ENABLE_TOOL_TESTS=1)")
        return

    status, _headers, body_json, raw_body = request_json_status(
        "GET", "/gemini/v1beta/fileSearchStores?pageSize=1"
    )
    assert 200 <= status < 300, (
        f"fileSearchStores list returned HTTP {status}: "
        f"{json_error_message(body_json, raw_body)}"
    )
    assert isinstance(body_json, dict), f"fileSearchStores list returned non-JSON: {raw_body!r}"
    print("file_search resource route pass-through OK")

    if not FILE_SEARCH_STORE_NAME:
        print("file_search query tool skipped (set APEX_GEMINI_NATIVE_FILE_SEARCH_STORE_NAME)")
        return

    payload = {
        "contents": [
            {
                "role": "user",
                "parts": [{"text": "Use file search and summarize the indexed document briefly."}],
            }
        ],
        "tools": [
            {
                "file_search": {
                    "file_search_store_names": [FILE_SEARCH_STORE_NAME],
                }
            }
        ],
        "generationConfig": {
            "temperature": 0,
            "maxOutputTokens": 512,
        },
    }
    body = request_tool_json("file_search", payload)
    if body is None:
        return
    candidate = first_candidate(body)
    assert extract_text(candidate).strip(), f"file_search returned empty text: {body}"
    print("file_search query tool pass-through OK")


def test_optional_computer_use_tool() -> None:
    if not is_enabled("computer_use", default=ENABLE_TOOL_TESTS):
        print("computerUse tool test skipped (set APEX_GEMINI_NATIVE_ENABLE_TOOL_TESTS=1)")
        return

    payload = {
        "contents": [
            {
                "role": "user",
                "parts": [
                    {
                        "text": (
                            "Open a web browser and navigate to https://example.com. "
                            "Return the first browser action you would take."
                        )
                    }
                ],
            }
        ],
        "tools": [
            {
                "computerUse": {
                    "environment": "ENVIRONMENT_BROWSER",
                }
            }
        ],
        "generationConfig": {
            "temperature": 0,
            "maxOutputTokens": 512,
        },
    }
    body = request_tool_json("computerUse", payload)
    if body is None:
        return
    candidate = first_candidate(body)
    assert contains_key(candidate, {"functionCall", "function_call"}), (
        f"computerUse response did not include a client-side function call: {body}"
    )
    print("computerUse tool first-turn pass-through OK")


def test_optional_deep_research_interactions() -> None:
    if not is_enabled("deep_research", default=False):
        print("deep_research interactions test skipped (set APEX_GEMINI_NATIVE_ENABLE_DEEP_RESEARCH=1)")
        return

    payload = {
        "input": (
            "In one short paragraph, research what Google Gemini Deep Research is. "
            "Keep the task small."
        ),
        "agent": DEEP_RESEARCH_AGENT,
        "background": True,
        "store": True,
    }
    status, _headers, body_json, raw_body = request_json_status(
        "POST", "/gemini/v1beta/interactions", payload
    )
    if not (200 <= status < 300):
        message = json_error_message(body_json, raw_body)
        if unsupported_tool_response("deep_research", status, message):
            return
        raise AssertionError(f"deep_research create returned HTTP {status}: {message}")
    assert isinstance(body_json, dict), f"deep_research create returned non-JSON: {raw_body!r}"
    interaction_id = body_json.get("id")
    assert isinstance(interaction_id, str) and interaction_id, (
        f"deep_research create returned no interaction id: {body_json}"
    )

    poll_id = interaction_id.removeprefix("interactions/")
    status, _headers, poll_json, poll_raw = request_json_status(
        "GET", f"/gemini/v1beta/interactions/{urllib.parse.quote(poll_id, safe='')}"
    )
    assert 200 <= status < 300, (
        f"deep_research get returned HTTP {status}: "
        f"{json_error_message(poll_json, poll_raw)}"
    )
    assert isinstance(poll_json, dict), f"deep_research get returned non-JSON: {poll_raw!r}"
    assert poll_json.get("id") or poll_json.get("name"), (
        f"deep_research get returned no id/name: {poll_json}"
    )
    print(f"deep_research interactions pass-through OK: {interaction_id}")


def test_usage_logged() -> None:
    body = request_json("GET", "/api/usage?limit=10", admin=True)
    rows = body.get("data") or []
    matching = [
        row
        for row in rows
        if row.get("router") == "gemini-native-real" and row.get("model") == MODEL
    ]
    assert matching, f"no usage row for Gemini native request: {body}"
    assert any((row.get("input_tokens") or 0) > 0 for row in matching), matching
    print("usage logging OK")


def main() -> int:
    tests = [
        test_get_model,
        test_generate_content,
        test_stream_generate_content,
        test_optional_google_search_tool,
        test_optional_code_execution_tool,
        test_optional_url_context_tool,
        test_optional_google_maps_tool,
        test_optional_file_search_routes,
        test_optional_computer_use_tool,
        test_optional_deep_research_interactions,
        test_usage_logged,
    ]
    for test in tests:
        try:
            test()
        except Exception as exc:
            print(f"{test.__name__} FAILED: {exc}", file=sys.stderr)
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
