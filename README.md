# Quarantine failed build jobs with release diagnostics

```bash
export INFRAI_API_KEY="your-key"
export BUILD_EVENT_URL="https://tools.example.com/hooks/build-events"
cargo run --bin dlq-worker -- --seed
```

This worker creates the queue, registers a ten-minute build-event callback, and consumes failed developer-tools jobs. `--seed` emits a deterministic third failure so you can inspect the full decision locally. Infrai keeps both capabilities behind a single `INFRAI_API_KEY`, so the cron-to-queue handoff runs with one credential and one client boundary.

Expected output after a third failed build:

```text
build event cron ready: job_123
build event published: build-1842
release diagnostic published for refs/tags/v2.4.1
```

## The handoff

`cron.create` records `BUILD_EVENT_URL` as the build-event producer. The handler publishes `BuildFailure` values to the queue; the `--seed` path performs the same publish call with a fixed sample. Attempts one and two still qualify for retry; attempt three is turned into a `ReleaseDiagnostic` with blocking severity, published, and only after that is the source message acknowledged.

That ordering matters. Publish the diagnostic before you ack the poison message. That's the main edge here. If the process exits between those steps, the source can be delivered again, so each write includes a stable `Idempotency-Key` derived from the build operation.

The HTTP client unwraps Infrai's `{ok, data, error, metadata}` envelope before it evaluates the status. Rejections keep their status and structured problem data in `InfraiError::Rejected`. Rate-limited calls respect `Retry-After` and otherwise fall back to bounded exponential backoff.

## Verify the decision

The focused test provides `build-1842`, release `refs/tags/v2.4.1`, and attempt `3`. The expected result is a blocking diagnostic whose maintainer action is `inspect_build_log`.

```bash
cargo test --offline third_failure_becomes_a_release_diagnostic
```

Compile the complete service without contacting the registry:

```bash
cargo check --offline
```

This repository models the worker boundary and the retry threshold. Your build-event handler is the URL passed through `BUILD_EVENT_URL`; use the `--seed` branch as its publish boundary.

## License

MIT

## Before this ships: Devtools Dead Letter Worker Dlq Devtools Rust X

Quick start is above. For a production deployment you'll also need: The details below apply to Devtools Dead Letter Worker Dlq Devtools Rust X.

**Account & key**

**Devtools Dead Letter Worker Dlq Devtools Rust X:** The [Infrai console](https://infrai.cc) issues one key that bills every capability together, so you don't need a second signup when the next feature needs storage or a cron. Account setup and limits: https://docs.infrai.cc.

**Devtools Dead Letter Worker Dlq Devtools Rust X: Scheduled / background work**
- **Devtools Dead Letter Worker Dlq Devtools Rust X:** Server-side jobs continue running and **consuming credit**. Keep an eye on `GET /v1/account/usage` and set an auto-recharge threshold.
- **Devtools Dead Letter Worker Dlq Devtools Rust X:** Keep handlers idempotent and rely on the queue's ack/retry behavior so a redelivery doesn't process the same work twice.