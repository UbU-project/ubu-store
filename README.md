# ubu-store

`ubu-store` is the async Rust SQLite canonical state and admission layer for UbU Phase 1.
It is the only Phase 1 repository allowed to mutate canonical local state directly.

## Scope

- SQLite persistence through `sqlx`.
- JSON payload storage for canonical objects and related records.
- Compartment labels and policy summaries as Phase 1 domain data carried in JSON payloads.
- Admission checks for UbU IDs, object types, prefix-to-type consistency, timestamps, Compartment labels, and provenance shape through `ubu_core` where available.
- Append-only log insertion API.
- Worker submissions are recorded separately and do not become canonical objects without admission.

## Explicit Phase 1 Limits

- Phase 1 supports Compartment labels and policy summaries, but it does not provide physical per-Compartment database separation.
- Phase 1 does not claim encrypted-at-rest storage.
- Append-only logs are enforced at the application layer only. There is no SQLite trigger or database-level guarantee preventing direct log updates in Phase 1.

Phase 2 may use separate SQLite files per Compartment and optional SQLite encryption.

## SQLx Policy

This scaffold uses dynamic `sqlx::query` and `sqlx::query_as` calls. It does not require a live
`DATABASE_URL` or committed `.sqlx` offline metadata. If compile-time checked SQL is introduced
later, the repo must either commit `.sqlx` metadata or document the CI database setup explicitly.

## Local Checks

```sh
cargo fmt --check
cargo clippy --all-targets
cargo test
```
