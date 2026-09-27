# Changelog

## Unreleased

- Add ordered, atomic object/Log batch admission with per-write envelopes, rollback, replay, and duplicate-key rejection.
- Share prepared Log admission between individual and batch writers without changing the individual writer.
- Pin core with the Phase 1b Container record and pure validation helpers.

## 0.1.0

- Initial async Rust SQLite scaffold for UbU Phase 1 canonical state and admission.
