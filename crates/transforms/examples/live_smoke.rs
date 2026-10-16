//! Explicit opt-in network probe. Sends only the public example.com label.
use anyhow::{ensure, Result};
use linkscope_engine::model::Entity;
use linkscope_transforms::Service;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<()> {
    let service = Service::new()?;
    let input = Entity {
        id: "public-example".into(),
        kind: "domain".into(),
        label: "example.com".into(),
        ..Default::default()
    };
    let mut failed = 0;
    for kind in ["dns", "rdap", "certificates"] {
        let started = std::time::Instant::now();
        match service.run(kind, &input, CancellationToken::new()).await {
            Ok(output) => println!(
                "{kind}: {} records, {} relationships, {} warnings in {:?}",
                output.patch.entities.len(),
                output.patch.relationships.len(),
                output.warnings.len(),
                started.elapsed()
            ),
            Err(error) => {
                failed += 1;
                println!("{kind}: {error:#} ({:?})", started.elapsed());
            }
        }
    }
    ensure!(
        failed == 0,
        "{failed} public-data lookup(s) failed; inspect the results above"
    );
    Ok(())
}
