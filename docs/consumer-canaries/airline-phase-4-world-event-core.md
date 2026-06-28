# Consumer Canary: Airline Phase 4 world-event-core

Crate:

- `world-event-core`

Candidate revision:

- `https://github.com/DeanStr/world-infra.git` revision
  `41049ae237fd5367a23494d2cdb3f9ef93895bfe`

Consumer: Airline.

Owner: Airline cycle/event owner, pending formal review.

Date: 2026-06-28.

Scope:

- `loco-app` has a dev-only canary that constructs an
  `EventEnvelope<i32, IncarnationId<Uuid>, Event>` from the existing
  `cycleCompleted` event ID and payload.
- The canary preserves Airline's current `world_instance_id` requirement,
  durable dedupe ID, WebSocket JSON, Redis fanout behavior, and runtime
  dependency closure.
- Airline cycle finalization, stale-world-instance rejection, fanout ambiguity
  handling, SQL tables, notification center, and delivery-version semantics
  remain product-owned.

Command run:

```sh
cargo test -p loco-app cycle_completed_metadata_fits_world_event_envelope
```

Result:

- Passed locally on 2026-06-28.
- The test compiled `world-event-core` from the exact remote git revision above.
- One focused test passed; 1037 unrelated `loco-app` tests were filtered.

Failures and disposition:

- None.

Release decision:

- Suitable as the Airline Phase 4 consumer canary for `world-event-core`.
