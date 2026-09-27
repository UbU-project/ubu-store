mod common;

use serde_json::{json, Value};
use ubu_core::{UbuId, VersionRef};
use ubu_store::{
    models::{
        log_record::{LogRecord, NewLogRecord},
        object_record::{NewObjectRecord, ObjectRecord},
    },
    queries::{self, BatchResult, BatchWrite},
    StoreError, UbuStore,
};
const NOW: &str = "2026-09-27T09:00:00Z";
fn id(kind: &str, n: u8) -> String {
    format!("{kind}_018f3c8e9b2a7c4d8f1e2a3b4c5d6e{n:02x}")
}
fn task(n: u8, title: &str) -> NewObjectRecord {
    let id = id("task", n);
    NewObjectRecord {
        id: id.clone(),
        object_type: "Task".into(),
        version: 1,
        status: "active".into(),
        compartment_label: "synthetic".into(),
        payload: json!({"id":id,"title":title,"status":"active","provenance":{"created_at":NOW,"authority_source":"user"}}),
        created_at: NOW.into(),
        updated_at: NOW.into(),
    }
}
fn object(record: NewObjectRecord, expected: VersionRef) -> BatchWrite {
    BatchWrite::Object {
        envelope: common::envelope_for(&record.id, expected),
        record,
    }
}
async fn store() -> UbuStore {
    let s = UbuStore::in_memory().await.unwrap();
    common::admit_object(s.pool(), task(1, "Repair the synthetic gate"))
        .await
        .unwrap();
    s
}
fn writes(malformed: bool) -> Vec<BatchWrite> {
    let origin = id("task", 1);
    let log_id = id("log", 1);
    let container_id = id("container", 1);
    let log = BatchWrite::Log {
        envelope: common::append_envelope(),
        record: NewLogRecord {
            id: log_id.clone(),
            event_type: "task_decomposed".into(),
            object_refs: json!([origin]),
            payload: json!({"container_id":container_id}),
            provenance: json!({"created_at":NOW,"authority_source":"user"}),
            created_at: NOW.into(),
        },
    };
    let mut children = Vec::new();
    for (n, title) in [
        (2, "Buy hinges"),
        (3, "Remove old hinges"),
        (4, "Hang the gate"),
    ] {
        let mut record = task(n, title);
        if n > 2 {
            record.payload["blocked_by"] = json!([id("task", n - 1)]);
        }
        if n == 4 && malformed {
            record.status = "moot".into();
            record.payload["status"] = "moot".into();
        }
        let mut write = object(record, VersionRef::Absent);
        // A later admission must see the child's version written earlier.
        if let BatchWrite::Object { envelope, .. } = &mut write {
            if n > 2 {
                envelope.observed_versions.insert(
                    UbuId::parse(id("task", n - 1)).unwrap(),
                    VersionRef::Version(1),
                );
            }
        }
        children.push(write);
    }
    let payload = json!({"id":container_id,"name":"Repair the synthetic gate","status":"active","origin_task_ref":origin,"origin_task_version":1,"mutation_reason":"decomposition","mutation_log_ref":log_id,"items":[
        {"ref":{"id":id("task",2),"object_type":"Task"},"summary":"Buy hinges"},
        {"ref":{"id":id("task",3),"object_type":"Task"},"summary":"Remove old hinges"},
        {"ref":{"id":id("task",4),"object_type":"Task"},"summary":"Hang the gate"}],"segment_split_points":[],"provenance":{"created_at":NOW,"authority_source":"user"}});
    serde_json::from_value::<ubu_core::core::Container>(payload.clone())
        .unwrap()
        .validate()
        .unwrap();
    let container = object(
        NewObjectRecord {
            id: container_id,
            object_type: "Container".into(),
            version: 1,
            status: "active".into(),
            compartment_label: "synthetic".into(),
            payload,
            created_at: NOW.into(),
            updated_at: NOW.into(),
        },
        VersionRef::Absent,
    );
    let mut origin = task(1, "Repair the synthetic gate");
    origin.status = "moot".into();
    origin.version = 2;
    origin.payload["status"] = "moot".into();
    origin.payload["moot_reason_code"] = "replaced_by_new_plan_structure".into();
    let mut result = vec![log];
    result.extend(children);
    result.push(container);
    result.push(object(origin, VersionRef::Version(1)));
    result
}
async fn counts(s: &UbuStore) -> (i64, i64, i64) {
    let p = s.pool();
    (
        sqlx::query_scalar("SELECT COUNT(*) FROM objects")
            .fetch_one(p)
            .await
            .unwrap(),
        sqlx::query_scalar("SELECT COUNT(*) FROM logs")
            .fetch_one(p)
            .await
            .unwrap(),
        common::ledger(p).await.len() as i64,
    )
}
async fn snapshot(s: &UbuStore) -> Vec<u8> {
    let objects: Vec<ObjectRecord> = sqlx::query_as("SELECT * FROM objects ORDER BY id")
        .fetch_all(s.pool())
        .await
        .unwrap();
    let logs: Vec<LogRecord> = sqlx::query_as("SELECT * FROM logs ORDER BY id")
        .fetch_all(s.pool())
        .await
        .unwrap();
    serde_json::to_vec(&(objects, logs, common::ledger(s.pool()).await)).unwrap()
}
async fn origin(s: &UbuStore) -> ObjectRecord {
    queries::get_current_state(s.pool(), &id("task", 1))
        .await
        .unwrap()
        .unwrap()
}
fn summaries(results: &[BatchResult]) -> Value {
    Value::Array(results.iter().map(|r|match r {
        BatchResult::Object(o)=>json!({"kind":"Object","id":o.id,"object_type":o.object_type,"version":o.version,"status":o.status}),
        BatchResult::Log(l)=>json!({"kind":"Log","id":l.id,"event_type":l.event_type}),
    }).collect())
}

#[tokio::test]
async fn sequential_admission_characterizes_the_half_built_structure() {
    let s = store().await;
    let before = counts(&s).await;
    let mut failure = None;
    for write in writes(true) {
        let result = match write {
            BatchWrite::Object { envelope, record } => {
                queries::admit_object(s.pool(), &envelope, record)
                    .await
                    .map(|_| ())
            }
            BatchWrite::Log { envelope, record } => {
                queries::append_log_entry(s.pool(), &envelope, record)
                    .await
                    .map(|_| ())
            }
        };
        if let Err(error) = result {
            failure = Some(error);
            break;
        }
    }
    let error = failure.unwrap();
    assert!(error.to_string().contains("moot_reason_code"));
    assert_eq!(before, (1, 0, 1));
    assert_eq!(counts(&s).await, (3, 1, 4));
    assert_eq!(origin(&s).await.status, "active");
    for n in [2, 3] {
        assert!(queries::get_current_state(s.pool(), &id("task", n))
            .await
            .unwrap()
            .is_some());
    }
    assert!(queries::get_current_state(s.pool(), &id("container", 1))
        .await
        .unwrap()
        .is_none());
    println!(
        "EVIDENCE[P1B37a_test1]={}",
        json!({"before":before,"after":counts(&s).await,"reason":error.to_string(),"origin_status":origin(&s).await.status,"orphan_children":2,"containers":0})
    );
}
#[tokio::test]
async fn six_writes_commit_in_request_order_and_later_writes_see_earlier_ones() {
    let s = store().await;
    let before = counts(&s).await;
    let result = queries::admit_batch(s.pool(), writes(false)).await.unwrap();
    assert_eq!(result.len(), 6);
    assert_eq!(before, (1, 0, 1));
    assert_eq!(counts(&s).await, (5, 1, 7));
    assert_eq!(result[0].as_log().unwrap().event_type, "task_decomposed");
    assert!(result[0].as_object().is_none());
    for (i, n) in [(1, 2), (2, 3), (3, 4)] {
        assert_eq!(result[i].as_object().unwrap().id, id("task", n));
        assert!(result[i].as_log().is_none());
    }
    assert_eq!(result[4].as_object().unwrap().id, id("container", 1));
    let o = result[5].as_object().unwrap();
    assert_eq!(o.id, id("task", 1));
    assert_eq!(o.version, 2);
    assert_eq!(o.status, "moot");
    println!(
        "EVIDENCE[P1B37a_test2]={}",
        json!({"before":before,"after":counts(&s).await,"results":summaries(&result)})
    );
}
#[tokio::test]
async fn malformed_third_child_rolls_back_objects_logs_and_receipts_byte_for_byte() {
    let s = store().await;
    let before = counts(&s).await;
    let original = snapshot(&s).await;
    let error = queries::admit_batch(s.pool(), writes(true))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("moot_reason_code"));
    assert_eq!(snapshot(&s).await, original);
    assert_eq!(origin(&s).await.status, "active");
    assert_eq!(counts(&s).await, (1, 0, 1));
    println!(
        "EVIDENCE[P1B37a_test3]={}",
        json!({"before":before,"after":counts(&s).await,"reason":error.to_string(),"byte_identical":true,"origin_status":origin(&s).await.status})
    );
}
#[tokio::test]
async fn stale_origin_on_last_write_rolls_back_every_prior_write() {
    let s = store().await;
    let batch = writes(false);
    let changed = task(1, "Renamed between read and batch");
    queries::admit_object(
        s.pool(),
        &common::envelope_for(&changed.id, VersionRef::Version(1)),
        changed,
    )
    .await
    .unwrap();
    let before = counts(&s).await;
    let original = snapshot(&s).await;
    let error = queries::admit_batch(s.pool(), batch).await.unwrap_err();
    assert!(
        matches!(&error,StoreError::PreconditionFailed {object_id,expected,actual} if object_id==&id("task",1)&&expected=="v1"&&actual=="v2")
    );
    assert_eq!(snapshot(&s).await, original);
    assert_eq!(counts(&s).await, (1, 0, 2));
    assert_eq!(origin(&s).await.status, "active");
    println!(
        "EVIDENCE[P1B37a_test4]={}",
        json!({"before":before,"after":counts(&s).await,"reason":error.to_string(),"byte_identical":true,"origin_status":origin(&s).await.status})
    );
}
#[tokio::test]
async fn identical_batch_replay_writes_nothing_and_preserves_origin_version() {
    let s = store().await;
    let batch = writes(false);
    let result = queries::admit_batch(s.pool(), batch.clone()).await.unwrap();
    let before = counts(&s).await;
    let original = snapshot(&s).await;
    let version = origin(&s).await.version;
    let changes: i64 = sqlx::query_scalar("SELECT total_changes()")
        .fetch_one(s.pool())
        .await
        .unwrap();
    let replay = queries::admit_batch(s.pool(), batch).await.unwrap();
    let after_changes: i64 = sqlx::query_scalar("SELECT total_changes()")
        .fetch_one(s.pool())
        .await
        .unwrap();
    assert_eq!(summaries(&result), summaries(&replay));
    assert_eq!(changes, after_changes);
    assert_eq!(snapshot(&s).await, original);
    assert_eq!(version, 2);
    assert_eq!(origin(&s).await.version, version);
    println!(
        "EVIDENCE[P1B37a_test5]={}",
        json!({"before":before,"after":counts(&s).await,"results":summaries(&replay),"origin_version_before":version,"origin_version_after":origin(&s).await.version,"new_writes":after_changes-changes})
    );
}
#[tokio::test]
async fn duplicate_keys_are_rejected_even_for_identical_writes_and_cross_writer_reuse() {
    for identical in [true, false] {
        let s = store().await;
        let original = snapshot(&s).await;
        let first = object(task(2, "First"), VersionRef::Absent);
        let mut second = if identical {
            first.clone()
        } else {
            object(task(3, "Different"), VersionRef::Absent)
        };
        if let (
            BatchWrite::Object {
                envelope: first, ..
            },
            BatchWrite::Object {
                envelope: second, ..
            },
        ) = (&first, &mut second)
        {
            second.idempotency_key = first.idempotency_key.clone();
        }
        assert!(matches!(
            queries::admit_batch(s.pool(), vec![first, second])
                .await
                .unwrap_err(),
            StoreError::Core(ubu_core::UbuError::IdempotencyKeyConflict { .. })
        ));
        assert_eq!(snapshot(&s).await, original);
    }
    let s = store().await;
    let original = snapshot(&s).await;
    let mut batch = writes(false);
    let key = match &batch[0] {
        BatchWrite::Log { envelope, .. } => envelope.idempotency_key.clone(),
        _ => unreachable!(),
    };
    if let BatchWrite::Object { envelope, .. } = &mut batch[1] {
        envelope.idempotency_key = key;
    }
    assert!(queries::admit_batch(s.pool(), batch).await.is_err());
    assert_eq!(snapshot(&s).await, original);
}
#[tokio::test]
async fn injected_ledger_failure_leaves_no_object_or_log_rows() {
    for log_first in [true, false] {
        let s = UbuStore::in_memory().await.unwrap();
        common::fail_ledger_inserts(s.pool()).await;
        let before = snapshot(&s).await;
        let mut batch = writes(false);
        if !log_first {
            batch.swap(0, 1);
        }
        let error = queries::admit_batch(s.pool(), batch).await.unwrap_err();
        assert!(error.to_string().contains("injected ledger failure"));
        assert_eq!(snapshot(&s).await, before);
        assert_eq!(counts(&s).await, (0, 0, 0));
    }
}
#[tokio::test]
async fn empty_batch_returns_no_results_and_writes_nothing() {
    let s = store().await;
    let before = snapshot(&s).await;
    assert!(queries::admit_batch(s.pool(), Vec::new())
        .await
        .unwrap()
        .is_empty());
    assert_eq!(snapshot(&s).await, before);
}
