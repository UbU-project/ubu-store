use ubu_store::init_store;

#[tokio::main]
async fn main() -> ubu_store::Result<()> {
    let _store = init_store("sqlite::memory:").await?;
    Ok(())
}
