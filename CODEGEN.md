# Codegen

This crate does not use generated Rust code in the initial scaffold.

SQLx compile-time query metadata is intentionally not generated for Phase 1 scaffolding because
the crate uses dynamic `sqlx::query` and `sqlx::query_as` calls.
