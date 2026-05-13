# 0000. Record Architecture Decisions

- **Status:** Accepted
- **Date:** project inception

## Context

We need a lightweight, durable record of the architectural decisions made on this project so that future contributors (human or AI) can understand *why* the system looks the way it does, not just *what* it is.

## Decision

We will use Architecture Decision Records (ADRs) as described by Michael Nygard. Each ADR is a short markdown file in `docs/adr/` numbered sequentially. New ADRs are added with the `/adr <title>` command.

Each ADR captures: Status, Date, Context, Decision, Consequences, Alternatives Considered.

## Consequences

- Decisions are discoverable and reviewable in PRs.
- `CONTEXT.md` links to relevant ADRs instead of duplicating reasoning.
- Superseded decisions stay on file with a "Superseded by NNNN" status note.
