# Canonical and candidate writer boundary (P1B-5)

The normative UBU-D0258 classification table is in `src/queries.rs`. Advisory
proposals live only in `advisory_candidates` as candidate state. Review decision
events carry `MutationEnvelope`: UBU-D0274's required actor, authority, Device,
policy observations, times, and idempotency fields are already envelope fields.
This provides provenance and retry safety without classifying the proposal as an
admitted object. Initial candidate storage also uses an envelope for idempotency.

## Public domain writers

All signatures are async; `Result` is the store's result type. Candidate writers
are in `src/candidates.rs`; existing writers remain in `src/queries.rs`. All are
re-exported by `api::admission`.

```rust
pub async fn admit_object(pool: &SqlitePool, envelope: &MutationEnvelope, record: NewObjectRecord) -> Result<ObjectRecord>;
pub async fn persist_universe_state(pool: &SqlitePool, envelope: &MutationEnvelope, state: &UniverseState, authority_source: AuthoritySource) -> Result<ObjectRecord>;
pub async fn append_log_entry(pool: &SqlitePool, envelope: &MutationEnvelope, record: NewLogRecord) -> Result<LogRecord>;
pub async fn store_external_reference(pool: &SqlitePool, envelope: &MutationEnvelope, record: NewExternalReferenceRecord) -> Result<ExternalReferenceRecord>;
pub async fn store_advisory_candidate(pool: &SqlitePool, envelope: &MutationEnvelope, candidate: AdvisoryCandidate) -> Result<CandidateRecord>;
pub async fn transition_advisory_candidate(pool: &SqlitePool, envelope: &MutationEnvelope, id: &AdvisoryCandidateId, observed_version: u64, next_state: CandidateLifecycleState, trigger: Option<ResurfaceTrigger>) -> Result<CandidateRecord>;
pub async fn reject_advisory_candidate(pool: &SqlitePool, envelope: &MutationEnvelope, id: &AdvisoryCandidateId, observed_version: u64, suppression: SuppressionRecord) -> Result<CandidateRecord>;
pub async fn admit_advisory_candidate(pool: &SqlitePool, envelope: &MutationEnvelope, id: &AdvisoryCandidateId, observed_version: u64, record: NewObjectRecord) -> Result<(CandidateRecord, ObjectRecord)>;
pub async fn store_plan(pool: &SqlitePool, record: NewPlanRecord) -> Result<PlanRecord>;
pub async fn store_calendar(pool: &SqlitePool, record: NewCalendarRecord) -> Result<CalendarRecord>;
pub async fn store_projection_preview(pool: &SqlitePool, record: NewProjectionPreviewRecord) -> Result<ProjectionPreviewRecord>;
pub async fn store_projection_result(pool: &SqlitePool, record: NewProjectionResultRecord) -> Result<ProjectionResultRecord>;
pub async fn store_worker_submission(pool: &SqlitePool, record: NewWorkerSubmissionRecord) -> Result<WorkerSubmissionRecord>;
```

Only the final five writers are envelope-exempt: derived artifacts, projection
state, and noncanonical worker submissions. Initialization, migration execution,
and transaction creation are infrastructure, not domain writers.

## Structural verification

```sh
rg -n 'pub (async )?fn|INSERT INTO|UPDATE |DELETE FROM|REPLACE INTO' src
rg -n 'pub use|mod queries|mod candidates' src/api src/lib.rs
rg -n 'prepare_mutation|admit_prepared_object|write_object_row|record_mutation|finish_mutation' src
rg -n 'SELECT|JOIN' src/queries.rs src/replay.rs src/recalculation.rs src/api
```

- `objects` has one insert/update implementation, private `write_object_row`.
  It requires an envelope and is reached only after shared admission preparation.
- `admit_object` and `admit_advisory_candidate` share `admit_prepared_object`, which
  preserves validation, payload-only replay, preconditions, and ordinary ledger
  recording. The candidate writer owns the single transaction spanning this
  helper, the candidate state/version update, and the linked decision event. A
  failure anywhere explicitly rolls back every write, including the ledger.
- `persist_universe_state` shares the same lower-level object writer. Logs and
  external references keep their existing envelope-required append paths.
- All eight envelope-aware writers reuse `prepare_mutation`, `record_mutation`,
  and `finish_mutation`; precondition and replay logic is not copied.
- All candidate changes occur in `advisory_candidates`. Every lifecycle change
  calls core's `transition` and writes an event via private `write_decision`.
- Candidate reads are exported through `api::review`, not `api::query`. Admitted
  queries select only canonical tables, never join candidate tables, and never
  return CandidateRecord. `get_current_state` returns None for a valid candidate
  id, which is intentionally not a canonical UbuId. Invalid unrelated ids still
  fail the existing parser. Canonical admission cannot accept an advcand_ id.
  The canonical `get_recorded_mutation` view also excludes candidate-only ledger
  payloads (negative result versions); private `lookup_mutation` retains the full
  Device-global ledger for replay and cross-writer conflict detection.
- Candidate payloads preserve Compartment labels and refs; event envelopes retain
  supplied policy observations. This ticket deliberately provides no Compartment
  filtering or policy enforcement.

## Literal readings and representation choices

- The supplied migration is unchanged in shape: three tables and three explicit
  indexes. It creates fresh storage; no legacy rows are guessed to be proposals
  or deleted. Earlier canonical rows do not identify their admission path.
- Candidate creation requires Proposed and version >= 1, not necessarily 1.
  SQLite bounds versions to i64::MAX and overflow fails atomically. created_at is
  proposed_at; updated_at is the operation envelope's recorded_time. Producer
  provenance inside the candidate is retained independently of storage provenance.
- Candidate-only ledger results use the negative candidate version; positive
  versions remain canonical objects and zero remains append-only log/xref results.
  Mutation keys stay global per Device across all writers. Candidate-only request
  fingerprints include operation and inputs; admission retains ordinary
  record.payload equality and one ordinary canonical ledger row. Admission replay
  additionally requires the matching event/candidate/observed-version/object link,
  preventing reuse of an unrelated ordinary admission as a candidate decision.
  Replays return current rows, retaining P1B-3's current-state replay convention.
- Decision ids are `canddec:` plus a compact JSON pair of Device and key. This is
  unambiguous and retry-stable without a new UUID dependency or registry entry.
  An event stores the observed candidate version separately because candidate ids
  cannot inhabit MutationEnvelope's canonical UbuId precondition map.
- Core's state machine decides all legality. The generic transition API rejects
  Admitted/Rejected destinations with DedicatedCandidateWriterRequired after
  checking core legality: these require the specialized writers' mandatory
  canonical mutation or suppression side effects. This restricts API routing,
  adds no lifecycle edges, and leaves all 17 core edges available across writers.
- Deferral/resurfacing/archive/rejection links identify decision events; admission
  links identify the resulting canonical object. Resurfacing links the immediately
  preceding deferral and supplied trigger. Existing evidence, reason, correction,
  and replacement refs are preserved. The fixed API has no new reason/evidence/
  replacement input; no such content is fabricated. Suppression is linked via the
  event's candidate id and that candidate's suppression key; the exact supplied
  migration has no separate suppression/replacement columns on decision events.
- Rejection validates suppression metadata against the candidate and envelope:
  actor and authority match; decided_at is effective_time, with recorded_time
  independently retained on the event. A missing candidate suppression key may be
  assigned from the rejection input; an existing key must match. Hashes/reason/
  retention are caller inputs validated by core's suppression builder. Duplicate
  suppression keys fail rather than overwrite durable correction metadata.
- review_queue includes only Proposed/Resurfaced, ordered by review_order (SQLite
  NULLs first), then created_at and candidate id for deterministic ties. Events
  are ordered by observed version, independent of timestamp ordering.
- Two prior tests depended on the removed bypass: the UniverseState provenance
  test now uses ordinary envelope-aware admission with its payload assertions
  preserved; the test asserting an active canonical candidate and zero envelopes
  is replaced by the opposite separation regression. Other prior assertions stay.
- P1B-3a literal choices for existing writers remain: log/xref replay compares only
  record.payload; UniverseState compares caller state and authority, excluding
  generated timestamps and database shell metadata; its provenance.created_at and
  row.updated_at come from envelope.created_time and recorded_time. No Device
  registry checks, policy-version enforcement, matching, or payload purge is added.
