use serde_json::json;
use ubu_core::id_registry::ObjectType;
use ubu_core::UbuId;
use ubu_store::models::external_reference_record::NewExternalReferenceRecord;
use ubu_store::{queries, UbuStore};

#[tokio::test]
async fn stores_and_queries_external_reference() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    queries::store_external_reference(
        store.pool(),
        NewExternalReferenceRecord {
            id: UbuId::new(ObjectType::ExternalReference).to_string(),
            source_type: "github_issue".to_owned(),
            source_id: "42".to_owned(),
            url: Some("https://example.invalid/issues/42".to_owned()),
            payload_hash: None,
            payload: json!({"title": "issue"}),
            created_at: "2026-06-10T14:30:00Z".to_owned(),
        },
    )
    .await
    .expect("xref stored");

    let refs = queries::query_external_references(store.pool(), Some("github_issue"))
        .await
        .expect("xrefs query");
    assert_eq!(refs.len(), 1);
}
