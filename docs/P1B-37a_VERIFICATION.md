# P1B-37a verification

P1B-37a landed in dependency order and every published dependency points at a
published predecessor. The landing revisions are:

| repository | branch | revision | role |
| --- | --- | --- | --- |
| `ubu-schemas` | `p1b-37a-container-and-batch` | `4974166aeaf344c43fed5c8ab10dfa1c9139bb7d` | Container schema, fixtures, generator reference fix and regression test |
| `ubu-core` | `p1b-37a-container-and-batch` | `c77c0a2d1c3e206b8c18c023bb8b4ee0d06eb2d0` | Container type, validation, segments and completion helper |
| `ubu-store` | `p1b-37a-container-and-batch` | `4066b8184403799fee031eb0660ee1bc12b3c1e8` | atomic batch writer and tests |
| `ubu-planning-kernel` | `p1b-37a-container-and-batch` | `84b6d0d9b621ca9a9df10034a8f8dc660baaca5d` | exact `ubu-core` re-pin |
| `ubu-github-adapter` | `p1b-37a-container-and-batch` | `4c7e3b6d31008a3bd64f28c303b8aa01bca3e2a0` | exact `ubu-core` re-pin |
| `ubu-orchestrator` | `p1b-37a-container-and-batch` | `cacedc881ac90e7543edb0e91c93aafe2e5c5146` plus lock-edge correction | exact five-package re-pin |

The store documentation follow-up is `c4addda4507a54bbfe2c5a837690272dbbbf998a`.
It changes documentation only and therefore is intentionally not the store
revision pinned by the orchestrator. The orchestrator pins the functional store
revision `4066b818...` above.

All commits were signed and pushed to their matching remote branches. The
landing order was schemas, core, store, planning-kernel and adapter, then
orchestrator. Kernel and adapter contain only their core re-pin. The
orchestrator contains no source change.

## Validation

The offline gates passed with the following test counts:

| repository | tests passed | clippy warnings before → after |
| --- | ---: | ---: |
| `ubu-schemas` | 2 Rust tests; 80 valid and 82 invalid fixtures; 2 Node regression tests; generated TypeScript and casing checks | 0 → 0 |
| `ubu-core` | 137 | 2 → 2 |
| `ubu-store` | 108, including 8 `admit_batch` tests | 0 → 0 |
| `ubu-planning-kernel` | 81 | 0 → 0 |
| `ubu-github-adapter` | 23 | 0 → 0 |
| `ubu-orchestrator` | 304 | 10 → 10 |

The warning counts use distinct compiler diagnostics keyed by warning code,
message and primary source span; repeated lib and lib-test emissions are
deduplicated. All tests and checks ran offline through the IPv4/IPv6-seccomp
wrapper. The store batch run is recorded in
`.p1b-37a-results/D-tests.log`; its network run is
`.p1b-37a-results/D-network.log`. The orchestrator network run is
`.p1b-37a-results/E-orchestrator-network.log`; it made no Internet-family
socket calls.

The four required probe outputs are reproduced verbatim below. The first is
the sequential partial-write characterization, followed by the valid batch,
rollback, stale-precondition rollback, and replay evidence emitted by the
regression tests.

```text
EVIDENCE[P1B37a_test1]={"after":[3,1,4],"before":[1,0,1],"containers":0,"origin_status":"active","orphan_children":2,"reason":"invalid JSON payload: UBU-D0227: task status `moot` requires moot_reason_code"}
```

```text
EVIDENCE[P1B37a_test2]={"after":[5,1,7],"before":[1,0,1],"results":[{"event_type":"task_decomposed","id":"log_018f3c8e9b2a7c4d8f1e2a3b4c5d6e01","kind":"Log"},{"id":"task_018f3c8e9b2a7c4d8f1e2a3b4c5d6e02","kind":"Object","object_type":"Task","status":"active","version":1},{"id":"task_018f3c8e9b2a7c4d8f1e2a3b4c5d6e03","kind":"Object","object_type":"Task","status":"active","version":1},{"id":"task_018f3c8e9b2a7c4d8f1e2a3b4c5d6e04","kind":"Object","object_type":"Task","status":"active","version":1},{"id":"container_018f3c8e9b2a7c4d8f1e2a3b4c5d6e01","kind":"Object","object_type":"Container","status":"active","version":1},{"id":"task_018f3c8e9b2a7c4d8f1e2a3b4c5d6e01","kind":"Object","object_type":"Task","status":"moot","version":2}]}
```

```text
EVIDENCE[P1B37a_test3]={"after":[1,0,1],"before":[1,0,1],"byte_identical":true,"origin_status":"active","reason":"invalid JSON payload: UBU-D0227: task status `moot` requires moot_reason_code"}
```

```text
EVIDENCE[P1B37a_test4]={"after":[1,0,2],"before":[1,0,2],"byte_identical":true,"origin_status":"active","reason":"precondition failed for `task_018f3c8e9b2a7c4d8f1e2a3b4c5d6e01`: expected v1, actual v2"}
```

The replay result, including the unchanged origin version, is:

```text
EVIDENCE[P1B37a_test5]={"after":[5,1,7],"before":[5,1,7],"new_writes":0,"origin_version_after":2,"origin_version_before":2,"results":[{"event_type":"task_decomposed","id":"log_018f3c8e9b2a7c4d8f1e2a3b4c5d6e01","kind":"Log"},{"id":"task_018f3c8e9b2a7c4d8f1e2a3b4c5d6e02","kind":"Object","object_type":"Task","status":"active","version":1},{"id":"task_018f3c8e9b2a7c4d8f1e2a3b4c5d6e03","kind":"Object","object_type":"Task","status":"active","version":1},{"id":"task_018f3c8e9b2a7c4d8f1e2a3b4c5d6e04","kind":"Object","object_type":"Task","status":"active","version":1},{"id":"container_018f3c8e9b2a7c4d8f1e2a3b4c5d6e01","kind":"Object","object_type":"Container","status":"active","version":1},{"id":"task_018f3c8e9b2a7c4d8f1e2a3b4c5d6e01","kind":"Object","object_type":"Task","status":"moot","version":2}]}
```

The existing orchestrator suite remains unchanged: 304 tests pass and its
source tree has no P1B-37a implementation change. The new store writer audit
row is:

```rust
//! | `admit_batch` | admitted canonical | Required per write: composes prepared object/Log writers in one transaction and owns commit or rollback for all of them. |
```

`BatchWrite`, `BatchResult` and `admit_batch` expose only domain records and
`SqlitePool`; no public signature exposes `Transaction`, `SqliteConnection` or
`PreparedMutation`. Those types remain private implementation details, verified
by the public API declarations and the writer audit.

The lockfile audit is byte-identical for schemas and core. Store, planning
kernel and adapter each change only the `ubu_core` git source from
`8d8d8e4...` to `c77c0a2...`. The orchestrator changes only these five git
sources: `ubu_core` to `c77c0a2...`, `ubu_store` to `4066b81...`, both planning
packages to `84b6d0d...`, and `ubu_github_adapter` to `4c7e3b6...`. There are
no new path dependencies and no `[patch]` entries. Existing internal workspace
path dependencies in the planning kernel were left unchanged.

## Decisions and limits

No implementation disagreement remained. The following explicit boundaries
were recorded during implementation:

1. `x-ubu-increasing-items` checks strict ordering in fixture validation; the
   upper bound depends on `items` length and is enforced by `ubu-core`.
2. Split points are exclusive contiguous segment boundaries.
3. Duplicate envelope keys within one batch are rejected explicitly, including
   identical payloads, instead of being silently treated as replay.
4. An empty batch returns no results and writes nothing.
5. The core completion helper returns `Result` for a child-state length
   mismatch; completion means every child is completed or moot.
6. The Container is defined and validated but no store endpoint, planner,
   projection or undo path creates or displays one in this ticket.
7. Container lifecycle status is enforced by schema and core validation, not by
   store admission.
8. The four supplied probe blocks are documented even though one prompt list
   calls them “three”.
9. The store docs-only follow-up is published after the functional revision;
   the orchestrator intentionally pins the functional revision.

The approved generator fix preserves the document owner while resolving local
references imported from another schema. Its two regression tests run offline
before TypeScript generation, preventing nested Task references from resolving
against the wrong schema document.
