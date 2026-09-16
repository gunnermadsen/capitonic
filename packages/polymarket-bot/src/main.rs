mod application;

// Keep the binary entry point intentionally small. Service wiring and domain
// behavior belong in focused application modules, not in this file.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    application::run().await
}
