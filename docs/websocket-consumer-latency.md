# Websocket consumer latency

## Purpose

Realtime market-data quality depends on continuously draining provider sockets. A provider may disconnect with `slow consumer: send buffer full` when its client does not return to the socket read quickly enough. This can happen before an application queue grows, so an empty internal queue does not prove that socket intake is healthy.

## Protected read boundary

The Polymarket orderbook data-frame path has one required sequence:

```text
socket.next()
→ classify required websocket control or data message
→ try_send the data message to the existing bounded channel
→ return to socket.next()
```

The successful data-frame path must not perform logging, metrics updates, JSON parsing, orderbook mutation, persistence, publication, allocation, or other optional accounting before enqueueing. Required ping, pong, close, timeout, and bounded-channel failure handling remain in the socket worker because they are part of transport correctness.

All data-frame processing and observability belongs after `io_receiver.recv()`. The existing bounded channel is the isolation boundary. If it fills, the strategy must preserve the existing explicit overflow failure and fresh-epoch reconnect; it must not use an unbounded queue or silently discard frames.

## Observability constraints

Metrics are necessary, but they must not compete with socket intake:

- Record frame, byte, freshness, inter-frame, queue, parse, apply, sample, persistence, and publication metrics on the consumer side.
- Do not add a metric that measures every metrics-lock acquisition; measuring the hot path that way adds more hot-path work.
- Build Prometheus output from a short in-memory snapshot and format it after releasing shared metrics locks.
- Treat received-versus-processed rate and queue depth as downstream evidence. Neither observes frames still waiting in the OS, TLS, or websocket decoder.
- Prefer existing bounded metrics and logs. Add new high-frequency instrumentation only when its runtime cost is explicitly bounded and verified.

## Review requirement

Any change between websocket data-frame receipt and successful channel enqueue must be treated as latency-sensitive. Reviewers must require a transport-correctness reason for the change and reject convenience logic in this boundary. Changes to downstream processing must preserve bounded backpressure, continuity-gap recording, fresh-epoch recovery, and automatic restart behavior.
