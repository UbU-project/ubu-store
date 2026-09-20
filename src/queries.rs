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
use sqlx::{Executor, Sqlite, SqlitePool};
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

/// Admit one canonical object mutation atomically with its envelope.
/// Device registration/trust and policy-version enforcement belong to later tickets.
/// Replay equality is deliberately defined over `record.payload` alone. A replay
/// reads the recorded target's current state; the ledger is not historical state.
pub async fn admit_object(
    pool: &SqlitePool,
    envelope: &MutationEnvelope,
    mut record: NewObjectRecord,
) -> Result<ObjectRecord> {
    let mut transaction = crate::transactions::begin(pool).await?;
    let result: Result<ObjectRecord> = async {
        envelope.validate()?;
        let key = envelope.mutation_key();
        let canonical_payload = String::from_utf8(canonical_payload_bytes(&record.payload))
            .expect("canonical JSON bytes are UTF-8");
        let replay: Option<(String, String)> = sqlx::query_as(
            "SELECT canonical_payload, result_object_id FROM mutation_envelopes
             WHERE origin_device_id = ? AND idempotency_key = ?",
        )
        .bind(key.origin_device_id.as_str())
        .bind(key.idempotency_key.as_str())
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some((previous_payload, object_id)) = replay {
            if previous_payload != canonical_payload {
                return Err(ubu_core::UbuError::IdempotencyKeyConflict {
                    origin_device_id: key.origin_device_id.as_str().to_owned(),
                    idempotency_key: key.idempotency_key.as_str().to_owned(),
                }.into());
            }
            return get_current_state(&mut *transaction, &object_id)
                .await?
                .ok_or_else(|| StoreError::RecordedMutationObjectMissing { object_id });
        }

        for (object_id, expected) in &envelope.observed_versions {
            let actual: Option<i64> = sqlx::query_scalar("SELECT version FROM objects WHERE id = ?")
                .bind(object_id.as_str())
                .fetch_optional(&mut *transaction)
                .await?;
            let matches = match (expected, actual) {
                (VersionRef::Absent, None) => true,
                (VersionRef::Version(expected), Some(actual)) => u64::try_from(actual).ok() == Some(*expected),
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

        let target = UbuId::parse(&record.id)?;
        let expected = envelope.observed_versions.get(&target).ok_or_else(|| {
            StoreError::MissingTargetPrecondition { object_id: record.id.clone() }
        })?;
        // Retain all existing record validation, including rejecting non-positive
        // supplied versions. The envelope, not a supplied positive version,
        // determines the version actually written.
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
                .execute(&mut *transaction)
                .await?;
            }
            VersionRef::Version(version) => {
                record.version = i64::try_from(*version).ok().and_then(|v| v.checked_add(1))
                    .ok_or_else(|| StoreError::ObjectVersionExhausted {
                        object_id: record.id.clone(), version: *version,
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
                .execute(&mut *transaction)
                .await?;
            }
        }
        sqlx::query(
            "INSERT INTO mutation_envelopes
             (origin_device_id, idempotency_key, envelope_json, canonical_payload,
              result_object_id, result_version, recorded_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(key.origin_device_id.as_str())
        .bind(key.idempotency_key.as_str())
        .bind(serde_json::to_string(envelope)?)
        .bind(canonical_payload)
        .bind(&record.id)
        .bind(record.version)
        .bind(envelope.recorded_time.to_string())
        .execute(&mut *transaction)
        .await?;

        // Read on the transaction's connection, avoiding a second pool checkout
        // (the store has one connection), then return only after commit succeeds.
        get_current_state(&mut *transaction, &record.id)
            .await?
            .ok_or_else(|| StoreError::RecordedMutationObjectMissing { object_id: record.id.clone() })
    }.await;

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

pub async fn append_log_entry(pool: &SqlitePool, record: NewLogRecord) -> Result<LogRecord> {
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
    .execute(pool)
    .await?;

    sqlx::query_as::<_, LogRecord>("SELECT * FROM logs WHERE id = ?")
        .bind(&record.id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
}

/// Look up an admission audit row by its complete device-scoped duplicate key.
pub async fn get_recorded_mutation(
    pool: &SqlitePool,
    key: &MutationKey,
) -> Result<Option<RecordedMutation>> {
    sqlx::query_as::<_, RecordedMutation>(
        "SELECT * FROM mutation_envelopes WHERE origin_device_id = ? AND idempotency_key = ?",
    )
    .bind(key.origin_device_id.as_str())
    .bind(key.idempotency_key.as_str())
    .fetch_optional(pool)
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
    state: &UniverseState,
    authority_source: AuthoritySource,
) -> Result<ObjectRecord> {
    let id = state.id.to_string();
    let current = get_current_state(pool, &id).await?.ok_or_else(|| {
        StoreError::InvalidPayload(format!(
            "cannot persist UniverseState `{id}`: no current version exists"
        ))
    })?;

    // Serialize the updated container; carry the schema_version and provenance
    // shell metadata that live alongside the canonical UniverseState payload.
    let mut payload = serde_json::to_value(state)?;
    let current_payload: Value = serde_json::from_str(&current.payload_json)?;
    if let Some(schema_version) = current_payload.get("schema_version") {
        payload["schema_version"] = schema_version.clone();
    }
    let now = UbuTimestamp::now_utc();
    payload["provenance"] = serde_json::to_value(Provenance {
        created_at: now,
        created_by: None,
        authority_source,
        source: None,
        source_refs: None,
    })?;

    let now = now.to_string();
    let record = NewObjectRecord {
        id: id.clone(),
        object_type: ObjectType::UniverseState.as_str().to_owned(),
        version: current.version + 1,
        status: current.status.clone(),
        compartment_label: current.compartment_label.clone(),
        payload,
        created_at: current.created_at.clone(),
        updated_at: now,
    };
    validate_object_record(&record)?;
    let payload_json = serde_json::to_string(&record.payload)?;

    sqlx::query(
        "UPDATE objects
        SET version = ?, status = ?, compartment_label = ?, payload_json = ?, updated_at = ?
        WHERE id = ?",
    )
    .bind(record.version)
    .bind(&record.status)
    .bind(&record.compartment_label)
    .bind(&payload_json)
    .bind(&record.updated_at)
    .bind(&record.id)
    .execute(pool)
    .await?;

    Ok(get_current_state(pool, &id)
        .await?
        .expect("updated object is readable"))
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

pub async fn store_external_reference(
    pool: &SqlitePool,
    record: NewExternalReferenceRecord,
) -> Result<ExternalReferenceRecord> {
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
    .execute(pool)
    .await?;

    sqlx::query_as::<_, ExternalReferenceRecord>("SELECT * FROM external_references WHERE id = ?")
        .bind(&record.id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
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
