# 이설 Architecture Overview

## Product purpose

이설 is a lightweight Windows Desktop Aide that makes the user's AI-assisted development queue visible with minimal attention cost. Its initial projects are Noctua and FGO. The compact overlay should let the user see each project, its current role and status, a short current-work identifier, and whether user attention is required.

## Product boundary

이설 is a presentation layer, not workflow authority. Canonical workflow state and evidence remain in the real project development systems: repositories, handoff documents, execution results, QA reports, test and commit evidence, and local runtime evidence.

```text
Noctua / FGO workflow state
        → local state projection
        → 이설 Desktop Aide
        → project status UI + overall character animation
```

## Current phases

- **Phase 1 — Windows Desktop Runtime:** COMPLETE / BASELINE FROZEN (`1fe8bd97be5e50b8935f6f23e40e1c5d008eaf98`).
- **Phase 2 — Development Queue Aide:** NEXT.
- **Later — Local producers/adapters:** selected after the queue runtime contract is stable.

The earlier assumption that a ChatGPT Browser Observer must be next is retired.

## Minimal Phase 2 model

The conceptual unit is a per-project current workflow snapshot. Candidate information is project, role, status, short label or tranche identifier, and freshness/update information if needed. For example only:

```text
Noctua / Execution / running / LA2-R-H03
FGO    / QA        / completed / LEGION-A1
```

Exact field names and status terminology remain unfrozen.

## Roles and statuses

Relevant roles are Prepare, QA, and Execution. Candidate statuses are `running`, `completed`, `waiting`, and `failed` or `blocked`. 이설 does not declare a larger workflow state machine unless implementation evidence requires it.

## UI principle

The default overlay is not a dashboard. Noctua and FGO must be glanceable simultaneously; detailed history and logs are not permanently displayed. One character provides an overall visual aide rather than a separate pet instance for each project.

## Visual-state principle

Project workflow state and physical sprite state are separate concepts. The overlay displays project-specific facts; the overall 이설 animation is derived from the project snapshots. An external producer must not select sprite rows directly.

Persistent completed status is distinct from a transient acknowledgement animation. Completion may briefly acknowledge, then the overall state is recalculated. A completed role must not make that acknowledgement loop indefinitely.

## Existing runtime coexistence

Phase 2 architecture must account for `manualState`, `idleVariety`, `reducedMotion`, `animationSpeed`, drag mechanics, AI-chat transient states, tray/settings events, and timer/reset behavior. Final precedence is deliberately not invented yet.

## State ownership direction

The current direction, not a frozen implementation, is:

```text
Rust backend authoritative runtime queue snapshot
        → PetWindow projection
        → queue overlay + derived overall animation
```

`AppSettings` remains preference storage and is not the default queue-state store.

## Persistence direction

External workflow state is transient by default. On restart, a previous `running` state is not automatically trusted; a producer should re-establish current state.

## Local transport direction

Loopback-only HTTP JSON is the primary candidate. Local WebSocket and Rust/Tauri-native IPC remain legitimate comparison options until architecture discovery closes the decision. Any input must be machine-local, avoid network-wide binding and unnecessary remote exposure, and avoid an enterprise auth/account system. Malformed JSON, unsupported methods, and excessive request bodies should be handled safely.

## Open architecture questions

- Where authoritative runtime queue state lives.
- The exact snapshot contract and status set, including whether idle is explicit or absence.
- Precedence with manual, chat, drag, and idle-variety state.
- Overall animation arbitration for simultaneous Noctua/FGO states and completion acknowledgement behavior.
- Compact overlay geometry and the Rust–PetWindow state/event boundary.
- Local transport, lifecycle, and fixed versus dynamic port if HTTP is selected.
- Bridge-startup failure behavior and stale producer handling.
- The minimal boundary that keeps producers and renderer decoupled.

These remain open until Prepare inspects the current implementation.
