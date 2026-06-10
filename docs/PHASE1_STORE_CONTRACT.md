# Phase 1 Store Contract

## Status

This document defines the Phase 1 storage/admission contract for `ubu-store`.

It is intentionally **not** a Phase 2 replication protocol, transport protocol, CRDT design, encrypted-storage design, or per-Compartment database layout. Its purpose is to preserve the local invariants Phase 2 replication can later consume without redefining canonical state semantics.

## Contract summary

Phase 1 `ubu-store` is the canonical local state admission and persistence boundary for UbU. It stores admitted objects, append-only log/store events, external references, snapshots, plans, calendars, worker submissions, and projection records in SQLite through `sqlx`.

The SQLite schema is an implementation detail of the Phase 1 local store. It is not the Phase 2 replication contract. Future replication must operate over explicit store-level records, admitted-object envelopes, log entries, provenance-bearing events, snapshots, and Compartment-aware export/import APIs, not by assuming raw SQLite table compatibility.

Phase 1 must preserve stable prefixed UUIDv7 object IDs, schema versions, object versions, timestamps with timezone offsets, provenance, authority source, object references, and logical Compartment metadata so that Phase 2 transport and replication can be added without redefining canonical state semantics.

Phase 1 does not implement multi-device replication, CRDT merging, encrypted per-Compartment SQLite files, remote wipe, or transport protocols. Those are Phase 2 concerns. Phase 1 only preserves the local invariants necessary for those mechanisms to be added later.

## Canonical admission boundary

`ubu-store` is the only Phase 1 component allowed to mutate canonical local state directly.

Other repos may submit records for admission, including:

- candidate objects;
- planning results;
- worker submissions;
- imported external references;
- GitHub projection previews and projection results;
- user action events;
- snapshots;
- logs.

Those submissions do not become canonical state until admitted by `ubu-store` according to the applicable schema, provenance, authority, Compartment, and validation rules.

## SQLite is not the replication contract

Phase 1 uses SQLite through `sqlx` as the exclusive local persistence backend.

The SQLite table layout is not a public compatibility contract for Phase 2 replication. It may change through migrations as the Phase 1 implementation evolves. Phase 2 replication must not assume raw SQLite table compatibility, row ordering, page layout, file copying, or direct table synchronization.

Future replication should consume explicit store-level records through `ubu-store` APIs or later-defined export/import envelopes. Export/import envelopes are intentionally not part of the first Phase 1 scaffold.

## Canonical mutation and append-only evidence

Canonical state changes must create, correspond to, or be reconstructable from append-only evidence records, except where an explicitly documented migration, repair, or compaction procedure says otherwise.

Phase 1 append-only behavior is enforced at the application layer. Phase 1 does not claim physical append-only database enforcement.

At minimum, store mutations should preserve:

- the admitted object or event ID;
- the object type or event type;
- the schema version;
- the object version, where applicable;
- the admitted timestamp;
- relevant observed/source timestamps, where applicable;
- the authority source;
- provenance;
- logical Compartment metadata;
- object references or causality references, where applicable.

## Stable IDs

Canonical object IDs must be stable, globally unique enough for future replication, and independent of local SQLite row IDs.

Phase 1 IDs use prefixed lowercase unhyphenated UUIDv7 strings. The suffix must satisfy the UUIDv7 version and variant positions.

Pattern template:

```regex
^<prefix>_[0-9a-f]{12}7[0-9a-f]{3}[89ab][0-9a-f]{15}$
```

Examples of canonical prefixes include:

```text
task_
obj_
plan_
log_
xref_
comp_
snap_
worker_
proj_
cal_
```

SQLite internal row IDs, if any, must not be exposed as canonical UbU object IDs.

## Time and ordering fields

All canonical timestamps must be RFC 3339 strings with explicit timezone offsets. Naive timestamps are invalid for store contract records.

The store should preserve enough ordering data for later replication-readiness without implementing a Phase 2 ordering protocol. Relevant fields may include:

- `created_at`;
- `observed_at`, where relevant;
- `admitted_at`;
- `updated_at`, where relevant;
- `schema_version`;
- `object_version`;
- object references or causality references, where relevant.

This is not a CRDT, vector-clock, or merge-semantics design. Those remain Phase 2 concerns.

## Provenance and authority

Externally sourced, generated, delegated, policy-derived, or user-authored records must preserve provenance sufficient for audit, explanation, correction, and later replication.

The Phase 1 `AuthoritySource` set is:

```text
user
user_override
delegated
automation_worker
policy
system
```

`user` represents ordinary user authority. `user_override` is reserved for explicit user override of another proposed, delegated, automated, policy, or system-derived action.

The store should not treat `policy` or `system` as autonomous user-equivalent authorities. They are implementation/enforcement sources whose admissibility remains constrained by UbU's user-sovereignty, Compartment, and provenance rules.

## Logical Compartment metadata

Phase 1 stores Compartment labels and policy summaries as logical metadata.

Phase 1 does **not** claim encrypted at-rest per-Compartment isolation. Phase 1 does **not** implement separate SQLite files per Compartment. Phase 1 does **not** implement Phase 2 replication policy enforcement.

Phase 2 may map Compartments to separate SQLite files, encrypted stores, replication streams, transport policies, or remote-wipe/retention mechanisms.

The Phase 1 store must preserve enough Compartment metadata for those Phase 2 mechanisms to be added without redefining the meaning of canonical records.

## Worker submissions and external projections

Automation workers, planning kernels, GitHub importers, GitHub projection executors, and future advisory processes must not directly mutate canonical state.

They may submit:

- candidate objects;
- worker submissions;
- projection previews;
- projection results;
- imported external references;
- diagnostic records;
- proposed log entries.

`ubu-store` admits or rejects those submissions according to the store contract and relevant schema contracts.

GitHub remains an external projection/source, not the canonical UbU state store.

## Phase 1 non-goals

The Phase 1 store contract does not define or implement:

- multi-device replication;
- libp2p transport;
- CRDT merge semantics;
- vector clocks;
- encrypted SQLite configuration;
- separate SQLite files per Compartment;
- remote wipe;
- heartbeat-based retention;
- transport-level authorization;
- raw SQLite file synchronization;
- StoreExportEnvelope or StoreImportEnvelope schemas in the first scaffold.

## Phase 2 TODOs

Phase 2 should define replication and transport mechanisms that consume the Phase 1 store contract rather than raw SQLite tables.

Likely Phase 2 follow-up work includes:

- explicit store export/import envelopes;
- Compartment-aware replication streams;
- Device/Zone-aware replication authority;
- conflict handling and merge diagnostics;
- encrypted local storage research;
- optional per-Compartment SQLite layout;
- retention and remote-wipe semantics;
- transport-level authentication and authorization.
