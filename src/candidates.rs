//! Candidate-state writers. Review records never enter admitted-state tables.

use serde::Serialize;
use serde_json::json;
use sqlx::{SqliteConnection, SqlitePool};
use ubu_core::{
    transition, AdvisoryCandidate, AdvisoryCandidateId, CandidateLifecycleState, MutationEnvelope,
    ResurfaceTrigger, RetentionPolicy, SuppressionDecision, SuppressionRecord,
};

use crate::errors::{Result, StoreError};
use crate::models::candidate_record::{CandidateDecisionEvent, CandidateRecord};
use crate::models::object_record::{NewObjectRecord, ObjectRecord};
use crate::queries::{
    admit_prepared_object, finish_mutation, prepare_mutation, record_mutation, MutationTarget,
};

fn spelling<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .expect("enum serializes")
        .as_str()
        .expect("string enum")
        .to_owned()
}

/// Collision-free, deterministic identity from the Device-global mutation key.
/// Review event ids are opaque and outside the admitted-object id registry.
fn decision_id(envelope: &MutationEnvelope) -> String {
    format!(
        "canddec:{}",
        serde_json::to_string(&(
            envelope.origin_device_id.as_str(),
            envelope.idempotency_key.as_str(),
        ))
        .expect("strings serialize")
    )
}

async fn read_candidate(connection: &mut SqliteConnection, id: &str) -> Result<CandidateRecord> {
    sqlx::query_as("SELECT * FROM advisory_candidates WHERE advisory_candidate_id = ?")
        .bind(id)
        .fetch_optional(connection)
        .await?
        .ok_or_else(|| StoreError::CandidateMissing { id: id.to_owned() })
}

fn sqlite_version(candidate: &AdvisoryCandidate) -> Result<i64> {
    i64::try_from(candidate.version).map_err(|_| StoreError::ObjectVersionExhausted {
        object_id: candidate.advisory_candidate_id.as_str().to_owned(),
        version: candidate.version,
    })
}

async fn observed_candidate(
    connection: &mut SqliteConnection,
    id: &AdvisoryCandidateId,
    observed_version: u64,
) -> Result<AdvisoryCandidate> {
    let record = read_candidate(connection, id.as_str()).await?;
    if u64::try_from(record.version).ok() != Some(observed_version) {
        return Err(StoreError::PreconditionFailed {
            object_id: id.as_str().to_owned(),
            expected: format!("v{observed_version}"),
            actual: format!("v{}", record.version),
        });
    }
    record.candidate()
}

/// Initial proposals carry an envelope for storage idempotency, while their
/// producer provenance remains in the full candidate payload. No canonical write.
pub async fn store_advisory_candidate(
    pool: &SqlitePool,
    envelope: &MutationEnvelope,
    candidate: AdvisoryCandidate,
) -> Result<CandidateRecord> {
    let mut transaction = crate::transactions::begin(pool).await?;
    let result = async {
        candidate.validate()?;
        if candidate.lifecycle_state != CandidateLifecycleState::INITIAL {
            return Err(StoreError::InvalidInitialCandidateState { state: candidate.lifecycle_state });
        }
        let payload = json!({"operation": "store_advisory_candidate", "candidate": candidate});
        let prepared = prepare_mutation(&mut transaction, envelope, &payload, MutationTarget::Candidate).await?;
        if let Some(replay) = prepared.replay {
            return read_candidate(&mut transaction, &replay.result_object_id).await;
        }
        let version = sqlite_version(&candidate)?;
        sqlx::query("INSERT INTO advisory_candidates
            (advisory_candidate_id, candidate_kind, lifecycle_state, version, suppression_key,
             review_order, payload_json, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(candidate.advisory_candidate_id.as_str())
            .bind(spelling(candidate.candidate_kind)).bind(spelling(candidate.lifecycle_state))
            .bind(version).bind(&candidate.suppression_key).bind(candidate.review_order)
            .bind(serde_json::to_string(&candidate)?)
            .bind(candidate.proposed_at.to_string()).bind(envelope.recorded_time.to_string())
            .execute(&mut *transaction).await?;
        record_mutation(&mut transaction, envelope, &prepared.canonical_payload,
            candidate.advisory_candidate_id.as_str(), -version).await?;
        read_candidate(&mut transaction, candidate.advisory_candidate_id.as_str()).await
    }.await;
    finish_mutation(transaction, result).await
}

/// Persist one legal decision and bump its candidate. All callers already checked
/// the observed version; core's transition function remains the sole edge matrix.
async fn write_decision(
    connection: &mut SqliteConnection,
    envelope: &MutationEnvelope,
    mut candidate: AdvisoryCandidate,
    next: CandidateLifecycleState,
    trigger: Option<ResurfaceTrigger>,
    resulting_object_id: Option<&str>,
) -> Result<CandidateRecord> {
    let from = candidate.lifecycle_state;
    let observed_version = sqlite_version(&candidate)?;
    candidate.lifecycle_state = transition(from, next, trigger)?;
    let version =
        observed_version
            .checked_add(1)
            .ok_or_else(|| StoreError::ObjectVersionExhausted {
                object_id: candidate.advisory_candidate_id.as_str().to_owned(),
                version: candidate.version,
            })?;
    candidate.version = version as u64;
    let event_id = decision_id(envelope);
    match next {
        CandidateLifecycleState::Deferred => candidate.links.deferral_ref = Some(event_id.clone()),
        CandidateLifecycleState::Resurfaced => {
            candidate.links.prior_deferral_ref = candidate.links.deferral_ref.clone();
            candidate.links.resurface_trigger = trigger;
            candidate.links.resurfacing_ref = Some(event_id.clone());
        }
        CandidateLifecycleState::Admitted => {
            candidate.links.admission_ref = resulting_object_id.map(str::to_owned)
        }
        CandidateLifecycleState::Rejected => candidate.links.rejection_ref = Some(event_id.clone()),
        CandidateLifecycleState::Archived => candidate.links.archive_ref = Some(event_id.clone()),
        // Existing replacement/correction references are preserved, not fabricated.
        _ => {}
    }
    candidate.validate()?;
    sqlx::query(
        "UPDATE advisory_candidates SET lifecycle_state = ?, version = ?, suppression_key = ?,
        payload_json = ?, updated_at = ? WHERE advisory_candidate_id = ? AND version = ?",
    )
    .bind(spelling(next))
    .bind(version)
    .bind(&candidate.suppression_key)
    .bind(serde_json::to_string(&candidate)?)
    .bind(envelope.recorded_time.to_string())
    .bind(candidate.advisory_candidate_id.as_str())
    .bind(observed_version)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "INSERT INTO candidate_decision_events
        (id, advisory_candidate_id, from_state, to_state, resurface_trigger,
         observed_candidate_version, envelope_json, resulting_object_id, recorded_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(event_id)
    .bind(candidate.advisory_candidate_id.as_str())
    .bind(spelling(from))
    .bind(spelling(next))
    .bind(trigger.map(spelling))
    .bind(observed_version)
    .bind(serde_json::to_string(envelope)?)
    .bind(resulting_object_id)
    .bind(envelope.recorded_time.to_string())
    .execute(&mut *connection)
    .await?;
    read_candidate(connection, candidate.advisory_candidate_id.as_str()).await
}

/// Admission and rejection require their dedicated writers, so a state-only
/// transition cannot bypass the canonical mutation or durable suppression record.
pub async fn transition_advisory_candidate(
    pool: &SqlitePool,
    envelope: &MutationEnvelope,
    id: &AdvisoryCandidateId,
    observed_version: u64,
    next_state: CandidateLifecycleState,
    trigger: Option<ResurfaceTrigger>,
) -> Result<CandidateRecord> {
    let mut transaction = crate::transactions::begin(pool).await?;
    let result = async {
        let payload = json!({"operation": "transition_advisory_candidate", "id": id,
            "observed_version": observed_version, "next_state": next_state, "trigger": trigger});
        let prepared = prepare_mutation(
            &mut transaction,
            envelope,
            &payload,
            MutationTarget::Candidate,
        )
        .await?;
        if let Some(replay) = prepared.replay {
            return read_candidate(&mut transaction, &replay.result_object_id).await;
        }
        let candidate = observed_candidate(&mut transaction, id, observed_version).await?;
        transition(candidate.lifecycle_state, next_state, trigger)?;
        if matches!(
            next_state,
            CandidateLifecycleState::Admitted | CandidateLifecycleState::Rejected
        ) {
            return Err(StoreError::DedicatedCandidateWriterRequired { state: next_state });
        }
        let result = write_decision(
            &mut transaction,
            envelope,
            candidate,
            next_state,
            trigger,
            None,
        )
        .await?;
        record_mutation(
            &mut transaction,
            envelope,
            &prepared.canonical_payload,
            id.as_str(),
            -result.version,
        )
        .await?;
        Ok(result)
    }
    .await;
    finish_mutation(transaction, result).await
}

/// Reject atomically with a suppression record and a first-class decision event.
/// Only caller-owned rejection inputs; provenance is taken from the envelope.
#[derive(Debug, Clone, Serialize)]
pub struct RejectionInput {
    pub rejection_reason_or_user_correction: String,
    pub retention_policy: RetentionPolicy,
    pub evidence_hashes_or_source_fingerprints: Vec<String>,
    pub suppression_key: Option<String>,
}

/// Build the durable suppression record inside the rejection transaction.
pub async fn reject_advisory_candidate(
    pool: &SqlitePool,
    envelope: &MutationEnvelope,
    id: &AdvisoryCandidateId,
    observed_version: u64,
    input: RejectionInput,
) -> Result<CandidateRecord> {
    let mut transaction = crate::transactions::begin(pool).await?;
    let result = async {
        let payload = json!({"operation": "reject_advisory_candidate", "id": id,
            "observed_version": observed_version, "input": input});
        let prepared = prepare_mutation(&mut transaction, envelope, &payload, MutationTarget::Candidate).await?;
        if let Some(replay) = prepared.replay {
            return read_candidate(&mut transaction, &replay.result_object_id).await;
        }
        let mut candidate = observed_candidate(&mut transaction, id, observed_version).await?;
        transition(candidate.lifecycle_state, CandidateLifecycleState::Rejected, None)?;
        // A proposal may acquire its suppression key at rejection, but an existing
        // key must not silently change. All remaining shape/provenance is retained.
        if let (Some(existing), Some(supplied)) = (&candidate.suppression_key, &input.suppression_key) {
            if existing != supplied {
                return Err(StoreError::SuppressionKeyConflict);
            }
        }
        if candidate.suppression_key.is_none() {
            candidate.suppression_key = Some(input.suppression_key.clone().unwrap_or_else(|| {
                let identity = json!({
                    "candidate_kind": candidate.candidate_kind,
                    "normalized_proposal": candidate.normalized_proposal,
                    "target_refs": candidate.target_refs,
                });
                String::from_utf8(ubu_core::canonical_payload_bytes(&identity))
                    .expect("canonical JSON is UTF-8")
            }));
        }
        let mut rejected = candidate.clone();
        rejected.lifecycle_state = CandidateLifecycleState::Rejected;
        let suppression = rejected.suppression_record(SuppressionDecision {
            deciding_actor_identity_id: envelope.actor_identity_id.clone(),
            authority_source: envelope.authority_source,
            decided_at: envelope.effective_time,
            rejection_reason_or_user_correction: input.rejection_reason_or_user_correction,
            retention_policy: input.retention_policy,
            evidence_hashes_or_source_fingerprints: input.evidence_hashes_or_source_fingerprints,
        })?;
        sqlx::query("INSERT INTO suppression_records (suppression_key, advisory_candidate_id, payload_json, decided_at)
            VALUES (?, ?, ?, ?)")
            .bind(&suppression.suppression_key).bind(id.as_str())
            .bind(serde_json::to_string(&suppression)?).bind(suppression.decided_at.to_string())
            .execute(&mut *transaction).await?;
        let result = write_decision(&mut transaction, envelope, candidate,
            CandidateLifecycleState::Rejected, None, None).await?;
        record_mutation(&mut transaction, envelope, &prepared.canonical_payload, id.as_str(), -result.version).await?;
        Ok(result)
    }.await;
    finish_mutation(transaction, result).await
}

/// One transaction, one ordinary canonical envelope ledger entry, and one linked
/// review decision. Canonical payload-only replay behavior remains unchanged.
pub async fn admit_advisory_candidate(
    pool: &SqlitePool,
    envelope: &MutationEnvelope,
    id: &AdvisoryCandidateId,
    observed_version: u64,
    record: NewObjectRecord,
) -> Result<(CandidateRecord, ObjectRecord)> {
    let mut transaction = crate::transactions::begin(pool).await?;
    let result = async {
        let prepared = prepare_mutation(
            &mut transaction,
            envelope,
            &record.payload,
            MutationTarget::Object,
        )
        .await?;
        if let Some(replay) = &prepared.replay {
            // An ordinary object replay is not proof that this candidate was admitted.
            let event: Option<CandidateDecisionEvent> =
                sqlx::query_as("SELECT * FROM candidate_decision_events WHERE id = ?")
                    .bind(decision_id(envelope))
                    .fetch_optional(&mut *transaction)
                    .await?;
            let matches = event.is_some_and(|event| {
                event.advisory_candidate_id == id.as_str()
                    && u64::try_from(event.observed_candidate_version).ok()
                        == Some(observed_version)
                    && event.to_state == "admitted"
                    && event.resulting_object_id.as_deref()
                        == Some(replay.result_object_id.as_str())
            });
            if !matches {
                return Err(StoreError::ReplayTargetMismatch {
                    object_id: id.as_str().to_owned(),
                    expected_table: "candidate_decision_events",
                });
            }
            let candidate = read_candidate(&mut transaction, id.as_str()).await?;
            let object =
                admit_prepared_object(&mut transaction, envelope, record, prepared).await?;
            return Ok((candidate, object));
        }
        let candidate = observed_candidate(&mut transaction, id, observed_version).await?;
        transition(
            candidate.lifecycle_state,
            CandidateLifecycleState::Admitted,
            None,
        )?;
        let object = admit_prepared_object(&mut transaction, envelope, record, prepared).await?;
        let candidate = write_decision(
            &mut transaction,
            envelope,
            candidate,
            CandidateLifecycleState::Admitted,
            None,
            Some(&object.id),
        )
        .await?;
        Ok((candidate, object))
    }
    .await;
    finish_mutation(transaction, result).await
}

/// Active review surface only. SQLite sorts absent review_order before numbers;
/// creation time breaks ties, then candidate id makes equal timestamps stable.
pub async fn review_queue(pool: &SqlitePool) -> Result<Vec<CandidateRecord>> {
    sqlx::query_as(
        "SELECT * FROM advisory_candidates WHERE lifecycle_state IN ('proposed', 'resurfaced')
        ORDER BY review_order ASC, created_at ASC, advisory_candidate_id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

pub async fn get_advisory_candidate(
    pool: &SqlitePool,
    id: &AdvisoryCandidateId,
) -> Result<Option<CandidateRecord>> {
    sqlx::query_as("SELECT * FROM advisory_candidates WHERE advisory_candidate_id = ?")
        .bind(id.as_str())
        .fetch_optional(pool)
        .await
        .map_err(Into::into)
}

pub async fn list_candidate_decision_events(
    pool: &SqlitePool,
    id: &AdvisoryCandidateId,
) -> Result<Vec<CandidateDecisionEvent>> {
    // Version order preserves decision order even for equal/backdated timestamps.
    sqlx::query_as(
        "SELECT * FROM candidate_decision_events WHERE advisory_candidate_id = ?
        ORDER BY observed_candidate_version ASC, id ASC",
    )
    .bind(id.as_str())
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

pub async fn find_suppression_record(
    pool: &SqlitePool,
    suppression_key: &str,
) -> Result<Option<SuppressionRecord>> {
    let payload: Option<String> = sqlx::query_scalar(
        "SELECT payload_json FROM suppression_records WHERE suppression_key = ?",
    )
    .bind(suppression_key)
    .fetch_optional(pool)
    .await?;
    payload
        .map(|payload| serde_json::from_str(&payload).map_err(Into::into))
        .transpose()
}
