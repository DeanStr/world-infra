# Consumer Canary: Chairman Phase 4 world-event-core

Crate:

- `world-event-core`

Candidate revision:

- `https://github.com/DeanStr/world-infra.git` revision
  `41049ae237fd5367a23494d2cdb3f9ef93895bfe`

Consumer: Chairman.

Owner: Chairman world-infra DRI.

Date: 2026-06-28.

Scope:

- `chairman-game-db` has a dev-only canary that constructs an
  `EventEnvelope<Uuid, NoIncarnation, serde_json::Value>` from existing
  `world.cycle_completed` outbox metadata.
- The canary preserves Chairman's current SQL inserts, outbox reads, WebSocket
  replay JSON, visibility rules, and runtime dependency closure.
- Chairman event payload schemas, event names, aggregate semantics, retention,
  and notification policy remain product-owned.

Command run:

```sh
cargo test -p chairman-game-db cycle_completed_outbox_metadata_fits_world_event_envelope
```

Result:

- Passed locally on 2026-06-28.
- The test compiled `world-event-core` from the exact remote git revision above.
- One focused test passed; 66 unrelated `chairman-game-db` tests were filtered.

Failures and disposition:

- None.

Release decision:

- Suitable as the Chairman Phase 4 consumer canary for `world-event-core`.
