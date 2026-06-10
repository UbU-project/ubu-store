# Contributing

Run these checks before opening a pull request:

```sh
cargo fmt --check
cargo clippy --all-targets
cargo test
```

Keep canonical state mutation inside this crate. Do not add planner semantics, GitHub API calls,
UI code, direct worker authority over canonical writes, DB encryption, or multi-device sync to
Phase 1.
