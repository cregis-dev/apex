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
Existing retry behavior for reqwest transport errors and configured HTTP status
codes is unchanged.

### Gate the new semantics by config version

Configuration versions `1` and `1.0` retain the historical behavior where
`request_ms` is not enforced. The gateway logs a warning explaining how to opt
in. Version `1.1` and later enforce `request_ms`, including channel overrides.

Newly generated and distributed configuration templates use version `1.1` and
`request_ms: 300000`. Existing values are never rewritten heuristically because
the gateway cannot distinguish an old template default from an operator's
explicit choice.

## Verification

- An oversized OpenAI-compatible response fails inside the Anthropic conversion
  buffer before the full body is accumulated.
- A response-header timeout sends exactly one non-idempotent upstream request,
  does not call fallback, and returns `504`.
- Version `1` and `1.0` disable the header deadline; version `1.1` enables global
  and channel-level values, while zero still means disabled.
- `apex init`, examples, installers, and timeout documentation agree on version
  `1.1` and the 300-second request-header timeout.
