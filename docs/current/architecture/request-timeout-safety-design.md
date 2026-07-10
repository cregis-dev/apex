# Request Timeout Safety Design

## Goal

Enforce upstream response-header timeouts without allowing oversized buffered
responses, duplicate non-idempotent requests, or upgrade regressions for
existing configuration files.

## Decisions

### Bound protocol-conversion buffers

The OpenAI-to-Anthropic non-streaming conversion must enforce
`MAX_UPSTREAM_BODY_BYTES` while collecting chunks, before copying a chunk into
the accumulator. The downstream usage wrapper remains responsible for turning
the resulting body error into the existing protocol-specific `502` response.

### Do not replay ambiguous header timeouts

An Apex-owned `request_ms` deadline can expire after the upstream accepted the
request. Because completion is then unknown, that timeout must return `504`
immediately. It must not retry the same channel or enter a fallback channel.
For non-read methods, reqwest transport errors that occur after connection
establishment are also treated as ambiguous and returned immediately. Clear
connection-establishment failures may still retry because the upstream did not
receive the request. Existing retry behavior for safe reads and configured HTTP
status codes is unchanged.

### Do not replay state-changing requests after success headers

Once the upstream has returned a successful response status, a later body read
failure is no longer safe to replay for `POST`, `PUT`, `DELETE`, and other
non-read methods. Those requests return the protocol-specific body-read error
immediately and do not retry the same channel or enter fallback. `GET` and
`HEAD` requests retain the existing body-failure retry behavior because
replaying them is safe.

### Classify body deadlines from the actual response

SSE responses continue to use `response_ms` as a per-chunk inactivity deadline.
Non-SSE responses use it as a total body-read deadline even when the request
contained `stream: true`, because an upstream may ignore the requested mode and
return ordinary JSON. Gemini native `streamGenerateContent` without SSE remains
the explicit exception: its chunked JSON response is a real stream and only the
per-chunk inactivity deadline applies.

### Gate the new semantics by config version

Configuration versions `1` and `1.0` retain the historical behavior where
`request_ms` is not enforced and non-SSE `response_ms` remains a per-chunk
inactivity deadline rather than a total body deadline. The gateway logs a
warning explaining how to opt in. Version `1.1` and later enforce `request_ms`,
including channel overrides, and enforce the total non-SSE body deadline.
This version split applies to both success and error response bodies.

Configuration versions must contain one or more numeric dot-separated
components. Invalid versions are rejected during configuration loading instead
of silently disabling timeout protection.

### Keep protocol and memory limits enforceable

Gemini-native HTTP `504` errors use the Google canonical status
`DEADLINE_EXCEEDED`. The SSE usage parser checks a line's size before copying or
parsing it, so a newline-terminated oversized chunk cannot bypass the 1 MiB line
limit.

Newly generated and distributed configuration templates use version `1.1` and
`request_ms: 300000`. Existing values are never rewritten heuristically because
the gateway cannot distinguish an old template default from an operator's
explicit choice.

## Verification

- An oversized OpenAI-compatible response fails inside the Anthropic conversion
  buffer before the full body is accumulated.
- A response-header timeout sends exactly one non-idempotent upstream request,
  does not call fallback, and returns `504`.
- A successful non-idempotent request whose body later fails is not replayed;
  idempotent `GET` and `HEAD` requests may still retry.
- A non-SSE response remains subject to the total body deadline even if the
  client requested streaming; Gemini chunked JSON streaming remains exempt.
- Version `1` and `1.0` disable the header and total-body deadlines; version
  `1.1` enables global and channel-level values, while zero still means disabled.
- Invalid configuration versions fail loading, oversized SSE lines are dropped
  before copying, and Gemini `504` responses report `DEADLINE_EXCEEDED`.
- `apex init`, examples, installers, and timeout documentation agree on version
  `1.1` and the 300-second request-header timeout.
