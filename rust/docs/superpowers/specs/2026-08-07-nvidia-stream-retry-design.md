# NVIDIA stream retry hybrid design

## Problem

The NVIDIA proxy logs a successful upstream response as soon as it receives HTTP headers and a meaningful SSE prefix. The rest of the response is consumed later by `stream_response`. If reqwest reports `error decoding response body` after that point, the error is emitted as an Anthropic SSE error event, but the outer retry loop can no longer run because `handle_messages` has already returned the downstream response.

The existing behavior is correct for a response that has already been partially delivered: restarting another model and appending it would produce a corrupt Anthropic conversation. It is too eager, however, for short streams whose body fails immediately after the first meaningful chunk.

## Decision

Use a hybrid stream handoff:

1. Keep the existing request-level retry loop, key rotation, model fallback, and `Accept-Encoding: identity` request header.
2. After receiving a successful streaming response, prefetch the body before returning an HTTP response to Claude Code.
3. During prefetch, retryable failures include a reqwest body error and an incomplete EOF before a completion marker. These failures are retried because no downstream bytes have been sent yet.
4. Handoff to live streaming as soon as either condition is met:
   - the buffered upstream body reaches 256 KiB; or
   - one second has elapsed after the first meaningful output while the upstream is still active.
5. If the upstream completes successfully before handoff, pass the buffered chunks to the existing SSE converter and return the converted response.
6. After handoff, preserve the current safety behavior: do not invoke another upstream request after partial downstream output. Emit an Anthropic `error` event and terminate the response so Claude Code does not receive two concatenated assistant replies.

This limits the added latency and memory use to short/early streams while making immediate body-decoding failures retryable. It does not pretend that a response already sent to the client can be rewound.

## Scope and invariants

- Only streaming requests change. Non-streaming JSON handling is unchanged.
- `max_retries` continues to cap total upstream attempts for one request.
- A prefetch body error follows the existing network-failure path and rotates to the next available key.
- A prefetch incomplete EOF follows the existing model-fallback path when another model is available; otherwise it continues retrying until the attempt budget is exhausted.
- No API key is written to logs or error bodies.
- The Anthropic SSE event order remains `message_start`, content events, `message_delta`, and `message_stop` for completed responses.

## Test design

Add an integration-style proxy test with a local Axum upstream that:

- returns one meaningful SSE chunk and then a body stream error on the first request;
- returns a completed SSE response on the second request;
- asserts that two upstream requests occurred and the downstream body contains the retry response.

Keep a complementary test for a stream that crosses the handoff window and then ends without a completion marker. It must assert one upstream request and an Anthropic SSE `error` event, proving that the proxy does not stitch a second response after downstream output has started.

Existing short successful-stream tests must continue to pass, proving that fully prefetched responses use the same converter and event format.

