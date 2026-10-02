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
- **Phase 2A — Development Queue Aide runtime/presentation:** implemented.
- **Phase 2B — local Git workflow tracker:** implemented for the fixed Noctua/FGO snapshot model.
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

Relevant roles are Prepare, QA, and Execution. Tracker product statuses are running and completed. Local Git completion evidence moves a running role to completed; an explicit user handoff moves a completed role to its fixed next running role. Execution also permits an explicit local COMPLETE confirmation. 이설 does not declare a larger workflow state machine.

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

## Tracker boundary and startup recovery

The tracker observes configured local Git checkouts only; it requires no GitHub API or token. A small checkpoint stores per-project causal anchors, recovery cursors, and a revision token, while QueueRuntime is transient presentation/acknowledgement state. Startup reconciles a pinned local Git snapshot into one final causally provable state before projecting it. It does not restore stale running state blindly or replay pre-start workflow history through the visible queue.

Git graph order is used within gpt_prompt; cross-repository correlation remains conservatively timestamp-based where the repositories do not share ancestry.
