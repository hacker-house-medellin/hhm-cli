# hhm-cli

flags-2-env operator CLI for Hacker House Medellín health, listing, and WebSocket event watching.

**Product:** Hacker House Medellín — Operations software for an entrepreneur coliving and coworking community.

Run rooms, desks, member stays, community events, access workflows, and day-to-day operations for a hacker house in Medellín, Colombia.

## Safety and production boundary

The bootstrap does not implement payments, identity verification, door-control hardware, or Colombian lodging compliance. Add those only after security and local regulatory review.

This repository is an executable bootstrap, not a production deployment. Before live
use, add authentication, tenant authorization, rate limits, durable migrations,
observability, backups, incident response, dependency review, and secret management.
## Examples

```bash
cargo run -- health
cargo run -- --api-url http://127.0.0.1:8080 list
cargo run -- watch
```

Precedence is `CLI > environment > schema default`. The CLI audits
`.cli-flags.toml`, rejects unknown options and parse errors, and crosses into typed
configuration once before network work.
