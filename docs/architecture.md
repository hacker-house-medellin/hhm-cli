# Architecture

Rust Hacker House Medellin CLI integrated through flags-2-env for safe operations workflows.

## Fleet

- `hhm-interfaces`
- `hhm-api`
- `hhm-mash-web`
- `hhm-leptos-web`
- `hhm-dioxus-web`
- `hhm-sync`
- `hhm-cli`
- `hhm-infra`
- `hacker-house-medellin-clients`
- `hacker-house-medellin-libs`
- `hacker-house-medellin.github.io`
- `hacker-house-medellin-monorepo`

Interfaces own wire formats; libraries own reusable domain behavior; clients consume versioned contracts; runtimes own deployment behavior; monorepos coordinate pinned revisions. Edge code is allowlisted and never a generic proxy.
