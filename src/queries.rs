//! State writer classification under UBU-D0258. Later tickets adding writers
//! must extend this audit table and preserve the canonical envelope boundary.
//!
//! | Writer | State category | Envelope and reason |
//! | --- | --- | --- |
//! | `admit_object` | admitted canonical | Required: creates/updates canonical objects. |
//! | `persist_universe_state` | admitted canonical | Required: bumps a canonical object's version. |
//! | `append_log_entry` | admitted canonical | Required: records an append-only fact. |
//! | `store_external_reference` | admitted canonical | Required: `xref_` is a registry object type. |
//! | `store_plan`, `store_calendar` | `derived_state` | Exempt: derived artifacts, not canonical admission. |
//! | `store_projection_preview`, `store_projection_result` | `projection_state` | Exempt: projection artifacts, not canonical admission. |
//! | `store_worker_submission` | noncanonical submission | Exempt: submissions require admission before becoming canonical, per CONTRACT.md. |
//! | `admit_candidate_object` | `candidate_state` (defective today) | Exempt: sole UBU-D0274 exception through a private writer; P1B-4 will separate candidate storage. |
//!
//! Exempt artifact storage does not authorize canonical recording, invalidation,
//! or publication mutations without an envelope. Device-registry consultation
//! and policy-version enforcement remain later work.

use serde_json::Value;
use sqlx::{Executor, Sqlite, SqliteConnection, SqlitePool, Transaction};
use ubu_core::core::UniverseState;
use ubu_core::id_registry::ObjectType;
use ubu_core::store::{
    canonical_payload_bytes, CandidateObject, MutationEnvelope, MutationKey, VersionRef,
};
use ubu_core::{AuthoritySource, Provenance, UbuId, UbuTimestamp};

use crate::admission::{
    object_type_from_str, validate_object_id_for_type, validate_object_record,
    validate_provenance_json,
};
use crate::errors::{Result, StoreError};
use crate::models::calendar_record::{CalendarRecord, NewCalendarRecord};
use crate::models::external_reference_record::{
    ExternalReferenceRecord, NewExternalReferenceRecord,
};
use crate::models::log_record::{LogRecord, NewLogRecord};
use crate::models::object_record::{NewObjectRecord, ObjectRecord};
use crate::models::plan_record::{NewPlanRecord, PlanRecord};
use crate::models::projection_record::{
    NewProjectionPreviewRecord, NewProjectionResultRecord, ProjectionPreviewRecord,
    ProjectionResultRecord,
};
use crate::models::recorded_mutation::RecordedMutation;
use crate::models::worker_submission_record::{NewWorkerSubmissionRecord, WorkerSubmissionRecord};
use crate::recalculation::validate_recalculation_trigger_payload;

/// The unchanged ledger has no table column. Positive result versions denote
/// objects; zero denotes an unversioned append result, whose id identifies its
/// log/xref kind. Keys remain global per Device, never scoped to this enum.
#[derive(Clone, Copy)]
enum MutationTarget {
    Object,
    Log,
    ExternalReference,
}

impl MutationTarget {
    fn table(self) -> &'static str {
        match self {
            Self::Object => "objects",
            Self::Log => "logs",
            Self::ExternalReference => "external_references",
        }
    }

    fn accepts(self, recorded: &RecordedMutation) -> bool {
        match self {
            Self::Object => recorded.result_version > 0,
            Self::Log | Self::ExternalReference => {
                let expected = match self {
                    Self::Log => ObjectType::LogEntry,
                    _ => ObjectType::ExternalReference,
                };
                recorded.result_version == 0
                    && UbuId::parse(&recorded.result_object_id)
                        .and_then(|id| id.require_object_type(expected))
                        .is_ok()
            }
        }
    }
}

struct PreparedMutation {
    canonical_payload: String,
    replay: Option<RecordedMutation>,
}

/// Validate and check the shared device-scoped ledger before any preconditions.
/// Payload equality is unchanged for admit_object and applies to record.payload
/// for append-only writers. A same-payload key cannot switch result tables.
async fn prepare_mutation(
    connection: &mut SqliteConnection,
    envelope: &MutationEnvelope,
    payload: &Value,
    target: MutationTarget,
) -> Result<PreparedMutation> {
    envelope.validate()?;
    let key = envelope.mutation_key();
    let canonical_payload = String::from_utf8(canonical_payload_bytes(payload))
        .expect("canonical JSON bytes are UTF-8");
    let replay = get_recorded_mutation(&mut *connection, &key).await?;
    if let Some(recorded) = &replay {
        if recorded.canonical_payload != canonical_payload {
            return Err(ubu_core::UbuError::IdempotencyKeyConflict {
                origin_device_id: key.origin_device_id.as_str().to_owned(),
                idempotency_key: key.idempotency_key.as_str().to_owned(),
            }
            .into());
        }
        if !target.accepts(recorded) {
            return Err(StoreError::ReplayTargetMismatch {
                object_id: recorded.result_object_id.clone(),
                expected_table: target.table(),
            });
        }
    } else {
        check_preconditions(connection, envelope).await?;
    }
    Ok(PreparedMutation {
        canonical_payload,
        replay,
    })
}

async fn check_preconditions(
    connection: &mut SqliteConnection,
    envelope: &MutationEnvelope,
) -> Result<()> {
    for (object_id, expected) in &envelope.observed_versions {
        let actual: Option<i64> = sqlx::query_scalar("SELECT version FROM objects WHERE id = ?")
            .bind(object_id.as_str())
            .fetch_optional(&mut *connection)
            .await?;
        let matches = match (expected, actual) {
            (VersionRef::Absent, None) => true,
            (VersionRef::Version(expected), Some(actual)) => {
                u64::try_from(actual).ok() == Some(*expected)
            }
            _ => false,
        };
        if !matches {
            return Err(StoreError::PreconditionFailed {
                object_id: object_id.as_str().to_owned(),
                expected: match expected {
                    VersionRef::Absent => "absent".to_owned(),
                    VersionRef::Version(version) => format!("v{version}"),
                },
                actual: actual.map_or_else(|| "absent".to_owned(), |version| format!("v{version}")),
            });
        }
    }
    Ok(())
}

async fn record_mutation(
    connection: &mut SqliteConnection,
    envelope: &MutationEnvelope,
    canonical_payload: &str,
    object_id: &str,
    version: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO mutation_envelopes
         (origin_device_id, idempotency_key, envelope_json, canonical_payload,
          result_object_id, result_version, recorded_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(envelope.origin_device_id.as_str())
    .bind(envelope.idempotency_key.as_str())
    .bind(serde_json::to_string(envelope)?)
    .bind(canonical_payload)
    .bind(object_id)
    .bind(version)
    .bind(envelope.recorded_time.to_string())
    .execute(connection)
    .await?;
    Ok(())
}

async fn finish_mutation<T>(transaction: Transaction<'_, Sqlite>, result: Result<T>) -> Result<T> {
    match result {
        Ok(record) => {
            transaction.commit().await?;
            Ok(record)
        }
        Err(error) => {
            transaction.rollback().await?;
            Err(error)
        }
    }
}

fn target_precondition(envelope: &MutationEnvelope, id: &UbuId) -> Result<VersionRef> {
    envelope.observed_versions.get(id).copied().ok_or_else(|| {
        StoreError::MissingTargetPrecondition {
            object_id: id.to_string(),
        }
    })
}

async fn read_object_result(
    connection: &mut SqliteConnection,
    object_id: &str,
) -> Result<ObjectRecord> {
    get_current_state(connection, object_id)
        .await?
        .ok_or_else(|| StoreError::RecordedMutationObjectMissing {
            object_id: object_id.to_owned(),
        })
}

/// Private shared object writer, reachable only after envelope checks on the
/// same transaction. Preserve existing record validation and created_at on updates.
async fn write_object_row(
    connection: &mut SqliteConnection,
    envelope: &MutationEnvelope,
    mut record: NewObjectRecord,
) -> Result<ObjectRecord> {
    let expected = target_precondition(envelope, &UbuId::parse(&record.id)?)?;
    validate_object_record(&record)?;
    let payload_json = serde_json::to_string(&record.payload)?;
    match expected {
        VersionRef::Absent => {
            record.version = 1;
            sqlx::query(
                "INSERT INTO objects
                 (id, object_type, version, status, compartment_label, payload_json, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&record.id)
            .bind(&record.object_type)
            .bind(record.version)
            .bind(&record.status)
            .bind(&record.compartment_label)
            .bind(&payload_json)
            .bind(&record.created_at)
            .bind(&record.updated_at)
            .execute(&mut *connection)
            .await?;
        }
        VersionRef::Version(version) => {
            record.version = i64::try_from(version)
                .ok()
                .and_then(|v| v.checked_add(1))
                .ok_or_else(|| StoreError::ObjectVersionExhausted {
                    object_id: record.id.clone(),
                    version,
                })?;
            sqlx::query(
                "UPDATE objects SET version = ?, status = ?, compartment_label = ?,
                 payload_json = ?, updated_at = ? WHERE id = ?",
            )
            .bind(record.version)
            .bind(&record.status)
            .bind(&record.compartment_label)
            .bind(&payload_json)
            .bind(&record.updated_at)
            .bind(&record.id)
            .execute(&mut *connection)
            .await?;
        }
    }
    read_object_result(connection, &record.id).await
}

/// Admit one canonical object mutation atomically with its envelope.
/// Device registration/trust and policy-version enforcement belong to later tickets.
/// Replay equality is deliberately defined over `record.payload` alone. A replay
/// reads the recorded target's current state; the ledger is not historical state.
pub async fn admit_object(
    pool: &SqlitePool,
    envelope: &MutationEnvelope,
    record: NewObjectRecord,
) -> Result<ObjectRecord> {
    let mut transaction = crate::transactions::begin(pool).await?;
    let result = async {
        let prepared = prepare_mutation(
            &mut transaction,
            envelope,
            &record.payload,
            MutationTarget::Object,
        )
        .await?;
        if let Some(replay) = prepared.replay {
            return read_object_result(&mut transaction, &replay.result_object_id).await;
        }
        let admitted = write_object_row(&mut transaction, envelope, record).await?;
        record_mutation(
            &mut transaction,
            envelope,
            &prepared.canonical_payload,
            &admitted.id,
            admitted.version,
        )
        .await?;
        Ok(admitted)
    }
    .await;
    finish_mutation(transaction, result).await
}

/// UBU-D0274 violation retained solely for `admit_candidate_object`: candidate
/// proposals are still inserted into canonical `objects` as active version 1.
/// P1B-4 will separate candidate_state. Do not use this writer for canonical mutations.
async fn insert_object_row_without_envelope(
    pool: &SqlitePool,
    record: NewObjectRecord,
) -> Result<ObjectRecord> {
    validate_object_record(&record)?;
    let payload_json = serde_json::to_string(&record.payload)?;

    sqlx::query(
        "INSERT INTO objects
        (id, object_type, version, status, compartment_label, payload_json, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&record.id)
    .bind(&record.object_type)
    .bind(record.version)
    .bind(&record.status)
    .bind(&record.compartment_label)
    .bind(&payload_json)
    .bind(&record.created_at)
    .bind(&record.updated_at)
    .execute(pool)
    .await?;

    Ok(get_current_state(pool, &record.id)
        .await?
        .expect("inserted object is readable"))
}

pub async fn admit_candidate_object(
    pool: &SqlitePool,
    candidate: CandidateObject,
    compartment_label: &str,
) -> Result<ObjectRecord> {
    let now = UbuTimestamp::now_utc().to_string();
    let record = NewObjectRecord {
        id: candidate.candidate_id,
        object_type: candidate.object_type,
        version: 1,
        status: "active".to_owned(),
        compartment_label: compartment_label.to_owned(),
        payload: candidate.payload,
        created_at: now.clone(),
        updated_at: now,
    };
    insert_object_row_without_envelope(pool, record).await
}

/// Append a canonical fact. The log id needs no object-version precondition;
/// all supplied read preconditions are enforced. Replay compares record.payload.
pub async fn append_log_entry(
    pool: &SqlitePool,
    envelope: &MutationEnvelope,
    record: NewLogRecord,
) -> Result<LogRecord> {
    let mut transaction = crate::transactions::begin(pool).await?;
    let result = async {
        let prepared = prepare_mutation(
            &mut transaction,
            envelope,
            &record.payload,
            MutationTarget::Log,
        )
        .await?;
        if let Some(replay) = prepared.replay {
            return sqlx::query_as::<_, LogRecord>("SELECT * FROM logs WHERE id = ?")
                .bind(replay.result_object_id)
                .fetch_one(&mut *transaction)
                .await
                .map_err(Into::into);
        }
        validate_object_id_for_type(&record.id, ObjectType::LogEntry.as_str())?;
        validate_provenance_json(&record.provenance)?;
        if record.event_type == "recalculation_requested" {
            validate_recalculation_trigger_payload(&record.payload)?;
        }
        UbuTimestamp::parse(&record.created_at)?;
        let object_refs_json = serde_json::to_string(&record.object_refs)?;
        let payload_json = serde_json::to_string(&record.payload)?;
        let provenance_json = serde_json::to_string(&record.provenance)?;

        sqlx::query(
            "INSERT INTO logs
        (id, event_type, object_refs_json, payload_json, provenance_json, created_at)
        VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&record.id)
        .bind(&record.event_type)
        .bind(&object_refs_json)
        .bind(&payload_json)
        .bind(&provenance_json)
        .bind(&record.created_at)
        .execute(&mut *transaction)
        .await?;

        record_mutation(
            &mut transaction,
            envelope,
            &prepared.canonical_payload,
            &record.id,
            0,
        )
        .await?;

        sqlx::query_as::<_, LogRecord>("SELECT * FROM logs WHERE id = ?")
            .bind(&record.id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(Into::into)
    }
    .await;
    finish_mutation(transaction, result).await
}

/// Look up an admission audit row by its complete device-scoped duplicate key.
pub async fn get_recorded_mutation<'e, E>(
    executor: E,
    key: &MutationKey,
) -> Result<Option<RecordedMutation>>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query_as::<_, RecordedMutation>(
        "SELECT * FROM mutation_envelopes WHERE origin_device_id = ? AND idempotency_key = ?",
    )
    .bind(key.origin_device_id.as_str())
    .bind(key.idempotency_key.as_str())
    .fetch_optional(executor)
    .await
    .map_err(Into::into)
}

pub async fn get_current_state<'e, E>(executor: E, id: &str) -> Result<Option<ObjectRecord>>
where
    E: Executor<'e, Database = Sqlite>,
{
    UbuId::parse(id.to_owned())?;
    sqlx::query_as::<_, ObjectRecord>("SELECT * FROM objects WHERE id = ?")
        .bind(id)
        .fetch_optional(executor)
        .await
        .map_err(Into::into)
}

/// Persist an updated [`UniverseState`] container as a new current version.
///
/// UniverseState is a single current-state object (UBU-D0241): after a Task's
/// effects are applied elsewhere (by the orchestrator via the pure `ubu-core`
/// applicator), the resulting container is persisted here as the new current
/// version. This is a current-version update — not an append of mutation deltas;
/// mutation history lives in Logs. The store owns UniverseState persistence, so
/// callers never write SQL against it directly. Authorized by UBU-D0242.
///
/// The four collections (`facts`, `numeric_values`, `set_memberships`,
/// `event_markers`) and the shell fields (`id`, `captured_at`, `source_summary`,
/// `confidence_summary`) round-trip losslessly. The existing `schema_version`
/// shell metadata is preserved, and the supplied [`Provenance::authority_source`]
/// is carried on the persisted payload, consistent with the other canonical
/// writes. Admission invariants (id-prefix, payload id match, provenance) are
/// re-checked before the write.
pub async fn persist_universe_state(
    pool: &SqlitePool,
    envelope: &MutationEnvelope,
    state: &UniverseState,
    authority_source: AuthoritySource,
) -> Result<ObjectRecord> {
    let mut transaction = crate::transactions::begin(pool).await?;
    let result = async {
        // Compare caller inputs, not regenerated timestamps or mutable database
        // shell metadata. Identical retries remain identical after the write.
        let request_payload =
            serde_json::json!({"state": state, "authority_source": authority_source});
        let prepared = prepare_mutation(
            &mut transaction,
            envelope,
            &request_payload,
            MutationTarget::Object,
        )
        .await?;
        if let Some(replay) = prepared.replay {
            return read_object_result(&mut transaction, &replay.result_object_id).await;
        }
        if target_precondition(envelope, &state.id)? == VersionRef::Absent {
            return Err(StoreError::PreconditionFailed {
                object_id: state.id.to_string(),
                expected: "existing version".to_owned(),
                actual: "absent".to_owned(),
            });
        }
        let current = read_object_result(&mut transaction, state.id.as_str()).await?;
        let mut payload = serde_json::to_value(state)?;
        let current_payload: Value = serde_json::from_str(&current.payload_json)?;
        if let Some(schema_version) = current_payload.get("schema_version") {
            payload["schema_version"] = schema_version.clone();
        }
        payload["provenance"] = serde_json::to_value(Provenance {
            created_at: envelope.created_time,
            created_by: None,
            authority_source,
            source: None,
            source_refs: None,
        })?;
        let record = NewObjectRecord {
            id: state.id.to_string(),
            object_type: ObjectType::UniverseState.as_str().to_owned(),
            version: current.version,
            status: current.status,
            compartment_label: current.compartment_label,
            payload,
            created_at: current.created_at,
            updated_at: envelope.recorded_time.to_string(),
        };
        let admitted = write_object_row(&mut transaction, envelope, record).await?;
        record_mutation(
            &mut transaction,
            envelope,
            &prepared.canonical_payload,
            &admitted.id,
            admitted.version,
        )
        .await?;
        Ok(admitted)
    }
    .await;
    finish_mutation(transaction, result).await
}

pub async fn get_object_history(pool: &SqlitePool, object_id: &str) -> Result<Vec<LogRecord>> {
    UbuId::parse(object_id.to_owned())?;
    let needle = format!("%{object_id}%");
    sqlx::query_as::<_, LogRecord>(
        "SELECT * FROM logs WHERE object_refs_json LIKE ? ORDER BY created_at ASC",
    )
    .bind(needle)
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

pub async fn query_active_tasks(pool: &SqlitePool) -> Result<Vec<ObjectRecord>> {
    sqlx::query_as::<_, ObjectRecord>(
        "SELECT * FROM objects WHERE object_type = ? AND status = ? ORDER BY updated_at DESC",
    )
    .bind(ObjectType::Task.as_str())
    .bind("active")
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

/// Append a canonical external reference. No target object version is required;
/// supplied read preconditions and the global per-Device replay key still apply.
pub async fn store_external_reference(
    pool: &SqlitePool,
    envelope: &MutationEnvelope,
    record: NewExternalReferenceRecord,
) -> Result<ExternalReferenceRecord> {
    let mut transaction = crate::transactions::begin(pool).await?;
    let result = async {
        let prepared = prepare_mutation(
            &mut transaction,
            envelope,
            &record.payload,
            MutationTarget::ExternalReference,
        )
        .await?;
        if let Some(replay) = prepared.replay {
            return sqlx::query_as::<_, ExternalReferenceRecord>(
                "SELECT * FROM external_references WHERE id = ?",
            )
            .bind(replay.result_object_id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(Into::into);
        }
        validate_object_id_for_type(&record.id, ObjectType::ExternalReference.as_str())?;
        UbuTimestamp::parse(&record.created_at)?;
        let payload_json = serde_json::to_string(&record.payload)?;

        sqlx::query(
            "INSERT INTO external_references
        (id, source_type, source_id, url, payload_hash, payload_json, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&record.id)
        .bind(&record.source_type)
        .bind(&record.source_id)
        .bind(&record.url)
        .bind(&record.payload_hash)
        .bind(&payload_json)
        .bind(&record.created_at)
        .execute(&mut *transaction)
        .await?;

        record_mutation(
            &mut transaction,
            envelope,
            &prepared.canonical_payload,
            &record.id,
            0,
        )
        .await?;

        sqlx::query_as::<_, ExternalReferenceRecord>(
            "SELECT * FROM external_references WHERE id = ?",
        )
        .bind(&record.id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(Into::into)
    }
    .await;
    finish_mutation(transaction, result).await
}

pub async fn query_external_references(
    pool: &SqlitePool,
    source_type: Option<&str>,
) -> Result<Vec<ExternalReferenceRecord>> {
    if let Some(source_type) = source_type {
        return sqlx::query_as::<_, ExternalReferenceRecord>(
            "SELECT * FROM external_references WHERE source_type = ? ORDER BY created_at DESC",
        )
        .bind(source_type)
        .fetch_all(pool)
        .await
        .map_err(Into::into);
    }

    sqlx::query_as::<_, ExternalReferenceRecord>(
        "SELECT * FROM external_references ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

pub async fn store_plan(pool: &SqlitePool, record: NewPlanRecord) -> Result<PlanRecord> {
    validate_object_id_for_type(&record.id, ObjectType::Plan.as_str())?;
    UbuTimestamp::parse(&record.created_at)?;
    let payload_json = serde_json::to_string(&record.payload)?;

    sqlx::query(
        "INSERT INTO plans (id, request_id, status, payload_json, created_at)
        VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&record.id)
    .bind(&record.request_id)
    .bind(&record.status)
    .bind(&payload_json)
    .bind(&record.created_at)
    .execute(pool)
    .await?;

    sqlx::query_as::<_, PlanRecord>("SELECT * FROM plans WHERE id = ?")
        .bind(&record.id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
}

pub async fn store_calendar(
    pool: &SqlitePool,
    record: NewCalendarRecord,
) -> Result<CalendarRecord> {
    validate_object_id_for_type(&record.id, ObjectType::Calendar.as_str())?;
    validate_object_id_for_type(&record.plan_id, ObjectType::Plan.as_str())?;
    UbuTimestamp::parse(&record.window_start)?;
    UbuTimestamp::parse(&record.window_end)?;
    UbuTimestamp::parse(&record.created_at)?;
    let payload_json = serde_json::to_string(&record.payload)?;

    sqlx::query(
        "INSERT INTO calendars (id, plan_id, window_start, window_end, payload_json, created_at)
        VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&record.id)
    .bind(&record.plan_id)
    .bind(&record.window_start)
    .bind(&record.window_end)
    .bind(&payload_json)
    .bind(&record.created_at)
    .execute(pool)
    .await?;

    sqlx::query_as::<_, CalendarRecord>("SELECT * FROM calendars WHERE id = ?")
        .bind(&record.id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
}

pub async fn query_recalculation_triggers(pool: &SqlitePool) -> Result<Vec<LogRecord>> {
    sqlx::query_as::<_, LogRecord>(
        "SELECT * FROM logs WHERE event_type = ? ORDER BY created_at DESC",
    )
    .bind("recalculation_requested")
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

pub async fn store_worker_submission(
    pool: &SqlitePool,
    record: NewWorkerSubmissionRecord,
) -> Result<WorkerSubmissionRecord> {
    validate_object_id_for_type(&record.id, ObjectType::AutomationWorker.as_str())?;
    object_type_from_str(&record.object_type)?;
    serde_json::from_str::<AuthoritySource>(&format!("\"{}\"", record.authority_source))?;
    UbuTimestamp::parse(&record.submitted_at)?;
    UbuTimestamp::parse(&record.created_at)?;
    let payload_json = serde_json::to_string(&record.payload)?;

    sqlx::query(
        "INSERT INTO worker_submissions
        (id, candidate_id, object_type, status, payload_json, authority_source, submitted_at, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&record.id)
    .bind(&record.candidate_id)
    .bind(&record.object_type)
    .bind(&record.status)
    .bind(&payload_json)
    .bind(&record.authority_source)
    .bind(&record.submitted_at)
    .bind(&record.created_at)
    .execute(pool)
    .await?;

    sqlx::query_as::<_, WorkerSubmissionRecord>("SELECT * FROM worker_submissions WHERE id = ?")
        .bind(&record.id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
}

pub async fn store_projection_preview(
    pool: &SqlitePool,
    record: NewProjectionPreviewRecord,
) -> Result<ProjectionPreviewRecord> {
    validate_object_id_for_type(&record.id, ObjectType::ProjectionPreview.as_str())?;
    UbuTimestamp::parse(&record.created_at)?;
    let payload_json = serde_json::to_string(&record.payload)?;

    sqlx::query(
        "INSERT INTO projection_previews (id, request_id, status, payload_json, created_at)
        VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&record.id)
    .bind(&record.request_id)
    .bind(&record.status)
    .bind(&payload_json)
    .bind(&record.created_at)
    .execute(pool)
    .await?;

    sqlx::query_as::<_, ProjectionPreviewRecord>("SELECT * FROM projection_previews WHERE id = ?")
        .bind(&record.id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
}

pub async fn store_projection_result(
    pool: &SqlitePool,
    record: NewProjectionResultRecord,
) -> Result<ProjectionResultRecord> {
    UbuId::parse(record.id.clone())?;
    validate_object_id_for_type(&record.preview_id, ObjectType::ProjectionPreview.as_str())?;
    UbuTimestamp::parse(&record.created_at)?;
    let payload_json = serde_json::to_string(&record.payload)?;

    sqlx::query(
        "INSERT INTO projection_results (id, preview_id, status, payload_json, created_at)
        VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&record.id)
    .bind(&record.preview_id)
    .bind(&record.status)
    .bind(&payload_json)
    .bind(&record.created_at)
    .execute(pool)
    .await?;

    sqlx::query_as::<_, ProjectionResultRecord>("SELECT * FROM projection_results WHERE id = ?")
        .bind(&record.id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
}
