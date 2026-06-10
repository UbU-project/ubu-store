use ubu_store::UbuStore;

#[tokio::test]
async fn initializes_in_memory_store() {
    let store = UbuStore::in_memory().await.expect("store initializes");
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM objects")
        .fetch_one(store.pool())
        .await
        .expect("objects table exists");
    assert_eq!(count.0, 0);
}
