# Canonical writer boundary (P1B-3a)

The normative UBU-D0258 classification table is in the module documentation of
`src/queries.rs`. No writer is classified differently from the ticket's table.

## Public domain writers

All signatures below are async; `Result` is `crate::errors::Result`.

```rust
pub async fn admit_object(pool: &SqlitePool, envelope: &MutationEnvelope, record: NewObjectRecord) -> Result<ObjectRecord>;
pub async fn persist_universe_state(pool: &SqlitePool, envelope: &MutationEnvelope, state: &UniverseState, authority_source: AuthoritySource) -> Result<ObjectRecord>;
pub async fn append_log_entry(pool: &SqlitePool, envelope: &MutationEnvelope, record: NewLogRecord) -> Result<LogRecord>;
pub async fn store_external_reference(pool: &SqlitePool, envelope: &MutationEnvelope, record: NewExternalReferenceRecord) -> Result<ExternalReferenceRecord>;

pub async fn store_plan(pool: &SqlitePool, record: NewPlanRecord) -> Result<PlanRecord>;
pub async fn store_calendar(pool: &SqlitePool, record: NewCalendarRecord) -> Result<CalendarRecord>;
pub async fn store_projection_preview(pool: &SqlitePool, record: NewProjectionPreviewRecord) -> Result<ProjectionPreviewRecord>;
pub async fn store_projection_result(pool: &SqlitePool, record: NewProjectionResultRecord) -> Result<ProjectionResultRecord>;
pub async fn store_worker_submission(pool: &SqlitePool, record: NewWorkerSubmissionRecord) -> Result<WorkerSubmissionRecord>;
pub async fn admit_candidate_object(pool: &SqlitePool, candidate: CandidateObject, compartment_label: &str) -> Result<ObjectRecord>;
```

The first four require envelopes. The next five write derived/projection artifacts
or noncanonical worker submissions and remain unchanged. The last is the
explicit UBU-D0274 candidate exception: it alone calls the private
`insert_object_row_without_envelope`. Its signature and behavior remain unchanged
until P1B-4. Thus the ticket's no-envelope-free-public-writer rule is interpreted
with its explicitly required candidate exception.

## Verification

Audit commands (from the repository root):

```sh
rg -n 'pub (async )?fn|INSERT INTO|UPDATE |DELETE FROM|REPLACE INTO' src
rg -n 'pub use|mod queries' src/api src/lib.rs
rg -n 'insert_object_row_without_envelope|write_object_row|prepare_mutation|record_mutation|finish_mutation' src
```

Review every DML statement and follow its callers, including public re-exports:

- `objects`: private `write_object_row` requires an envelope and is called only
  from `admit_object` and `persist_universe_state`, after shared admission checks
  on the same transaction. The candidate-only private writer is the sole exception.
- `logs`: the only insertion is inside envelope-required `append_log_entry`.
- `external_references`: the only insertion is inside envelope-required
  `store_external_reference`.
- `mutation_envelopes`: one private `record_mutation` implementation serves all
  four canonical writers. `prepare_mutation`, `check_preconditions`, and
  `finish_mutation` provide one shared implementation of replay checks,
  preconditions, commit, and rollback. Every writer reads/writes on its transaction.
- `src/api/admission.rs` only re-exports these functions; it adds no bypass.
  Store initialization, migration execution, and transaction creation are
  infrastructure, not additional domain mutation APIs.

Review the diff against `056cfde` to confirm that the six exempt public writers,
the private candidate writer, migrations, Cargo manifest, and lockfile are
unchanged. Run the complete test suite, including the cross-writer conflict and
post-write failure rollback tests.

## Literal readings

- Idempotency keys are global per Device across all tables. Payload comparison
  occurs before target-kind checking and preconditions. Logs and external
  references use `record.payload`, retaining P1B-3's payload-only comparison.
- UniverseState compares the serialized caller state and supplied authority
  source, excluding database-derived shell metadata and generated timestamps.
  Its stored provenance uses `envelope.created_time`; `updated_at` uses
  `envelope.recorded_time`. The original object creation time is preserved.
- The unchanged ledger stores `result_version = 0` for append-only results;
  positive versions identify canonical object results. Canonical log/xref id
  prefixes distinguish unversioned result tables. An identical-payload key
  reused for an incompatible result table fails with `ReplayTargetMismatch`
  rather than inserting another mutation or reading the wrong table.
- Replays return current object state or the recorded append-only row. An
  existing ledger row survives conflicts; failed fresh keys leave no ledger row.
- No Device-registry or policy-version enforcement is added.
