use sqlx::SqlitePool;
use ubu_core::id_registry::ObjectType;
use ubu_core::store::CandidateObject;
use ubu_core::{AuthoritySource, UbuId, UbuTimestamp};

use crate::admission::{
    object_type_from_str, validate_object_id_for_type, validate_object_record,
    validate_provenance_json,
};
use crate::errors::Result;
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
use crate::models::worker_submission_record::{NewWorkerSubmissionRecord, WorkerSubmissionRecord};
use crate::recalculation::validate_recalculation_trigger_payload;

pub async fn admit_object(pool: &SqlitePool, record: NewObjectRecord) -> Result<ObjectRecord> {
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
    admit_object(pool, record).await
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

pub async fn get_current_state(pool: &SqlitePool, id: &str) -> Result<Option<ObjectRecord>> {
    UbuId::parse(id.to_owned())?;
    sqlx::query_as::<_, ObjectRecord>("SELECT * FROM objects WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(Into::into)
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
