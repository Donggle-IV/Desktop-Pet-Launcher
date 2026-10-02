# 이설 Development Instructions

## Product and repository authority

- The human-facing product and character name is **이설**. `a-10` is an internal compatibility identifier: do not rename its pet id, bundled paths, saved-selection identifiers, or asset paths merely for the name change.
- Production artwork remains Codex Pet V1 unless explicitly requested.
- Current repository code and the actual Git HEAD are authoritative. Inspect the AS-IS implementation before architectural changes; do not prefer old conversation assumptions to current code.
- Before work, check the branch, HEAD, dirty state, and repository-local instructions.

## Phase boundaries

- Phase 1, Windows Desktop Runtime, is **COMPLETE / BASELINE FROZEN** at `1fe8bd97be5e50b8935f6f23e40e1c5d008eaf98`. Do not redesign it unless Phase 2 directly requires it.
- Phase 2 is a compact Development Queue Aide: show current AI-assisted workflow snapshots for Noctua and FGO, including project, role, status, short work/tranche label, and whether user attention is required.
- Relevant roles are Prepare, QA, and Execution. Default UI prioritizes glanceability. 이설 animation is an overall queue summary, never workflow authority.

## Architecture constraints

- 이설 is a presentation layer. Canonical workflow evidence remains in Noctua/FGO repositories, handoffs, execution and QA reports, tests, commits, and runtime evidence.
- Use per-project current workflow snapshots; do not build queue history, dependency graphs, parent/child models, generalized orchestration, cloud services, accounts, or telemetry.
- Separate workflow semantics from physical animation mechanics. `running-left` and `running-right` are not external workflow semantics, and `PetState` is not an external contract without explicit architectural justification.
- Settings are preferences; queue state is runtime fact. Do not automatically persist external queue state in `AppSettings`, and do not restore stale `running` state after restart.
- State input must be machine-local. Loopback HTTP is a candidate, not a frozen choice. Do not implement browser, Codex, IDE, Noctua, or FGO producer integrations during core Phase 2 unless explicitly requested.

## Existing behavior and workflow

- Preserve `manualState`, `idleVariety`, `reducedMotion`, `animationSpeed`, drag behavior, AI-chat transient states, custom pet support, settings persistence, and production artwork unless an approved architecture change explicitly requires otherwise.
- Default flow: Prepare → Execution → Prepare → Execution. Independent QA is optional when risk justifies it. Do not create an orchestration framework for this project.
- Use existing frontend and Rust quality gates as applicable. Packaging-sensitive changes require a Tauri build. Never report unexecuted validation as passing, and distinguish automated evidence from user manual verification.

## Scope protection

Unless separately requested, do not undertake sprite redesign or V2 migration, framework migration, broad refactors, workflow engine work, cloud backend, telemetry, accounts, remote server, browser extension, Codex/IDE integration, Noctua/FGO-specific producers, internal `a-10` migration, or release publication.
