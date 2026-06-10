# Contract

`ubu-store` owns canonical local state mutation for UbU Phase 1.

## Admission

Canonical objects must pass admission before insertion:

- ID must be a valid `ubu_core::UbuId`.
- Declared `object_type` must be known to `ubu_core`.
- ID prefix must match the declared object type. For example, `Task` requires `task_`.
- Compartment label must parse as `ubu_core::CompartmentLabel`.
- Created and updated timestamps must parse as `ubu_core::UbuTimestamp`.
- Provenance, where accepted by an API, must deserialize as `ubu_core::Provenance`.

Worker submissions are never canonical writes by themselves. They must be converted through
admission before an object row is inserted.

## Storage

Payloads are stored as JSON strings in SQLite for Phase 1 flexibility. The schema is intentionally
minimal and migration-driven.

Compartment labels are first-class columns on canonical objects. Policy summaries are Phase 1
domain data carried in JSON payloads rather than separate physical storage boundaries.

## Append-Only Logs

Logs are append-only by API: this crate exposes insertion and read paths, not mutation paths.
This is enforced at the application layer only in Phase 1. There is no DB-level trigger or SQLite
constraint preventing direct updates to `logs`.

## Non-Claims

Phase 1 does not claim encrypted-at-rest storage or physical per-Compartment database isolation.
Phase 2 may introduce separate SQLite files per Compartment and optional SQLite encryption.
