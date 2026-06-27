# Chairman Product Hardening Tracker

This tracker keeps Airline-inspired product hardening separate from first-wave
shared crates.

## Local-Only Work

- contract gates for OpenAPI and WebSocket events;
- scoped SQL review or lint posture;
- static web runtime-config and CSP hardening;
- operator dead-letter workflows;
- upload/media safety if user-owned assets are introduced;
- world deletion/reset safety if destructive operations are exposed;
- release-candidate runbook and rollback criteria;
- session/world context UX.

These items must not be promoted to shared crates until Chairman has local
implementation evidence and Airline confirms a matching consumption path.
