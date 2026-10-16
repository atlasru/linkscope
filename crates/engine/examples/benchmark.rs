use linkscope_engine::{model::*, storage, Graph};
use std::{path::Path, time::Instant};

fn measure<T>(name: &str, operation: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let result = operation();
    println!("{name}: {:.3} ms", start.elapsed().as_secs_f64() * 1000.0);
    result
}

fn memory(label: &str) {
    #[cfg(windows)]
    {
        let script = format!("Get-Process -Id {} | Select-Object WorkingSet64,PeakWorkingSet64 | ConvertTo-Json -Compress", std::process::id());
        if let Ok(output) = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
        {
            if let Ok(values) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                if let (Some(working), Some(peak)) = (
                    values["WorkingSet64"].as_u64(),
                    values["PeakWorkingSet64"].as_u64(),
                ) {
                    println!(
                        "memory {label}: working={:.1} MiB peak={:.1} MiB",
                        working as f64 / 1048576.0,
                        peak as f64 / 1048576.0
                    );
                }
            }
        }
    }
    #[cfg(not(windows))]
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status
            .lines()
            .filter(|line| line.starts_with("VmRSS:") || line.starts_with("VmHWM:"))
        {
            println!("memory {label}: {line}");
        }
    }
}

fn main() -> anyhow::Result<()> {
    let count = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse::<usize>().ok())
        .unwrap_or(100_000);
    let edges = count * 5;
    anyhow::ensure!(count >= 100, "Use at least 100 entities");
    println!("LinkScope engine reproducible benchmark; entities={count}; relationships={edges}; deterministic ring chords");
    println!(
        "platform={}/{}; profile=release recommended",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let patch = measure("dataset generation", || Patch {
        entities: (0..count)
            .map(|index| Entity {
                id: format!("n{index}"),
                kind: "Record".into(),
                label: format!("Record {index}"),
                x: (index % 1000) as f64 * 20.0,
                y: (index / 1000) as f64 * 20.0,
                properties: [("ordinal".into(), serde_json::Value::from(index))]
                    .into_iter()
                    .collect(),
                ..Entity::default()
            })
            .collect(),
        relationships: (0..edges)
            .map(|index| Relationship {
                id: format!("e{index}"),
                source: format!("n{}", index % count),
                target: format!("n{}", (index % count + (index / count + 1) * 7919) % count),
                kind: "related".into(),
                directed: index % 2 == 0,
                ..Relationship::default()
            })
            .collect(),
        ..Patch::default()
    });
    let mut graph = Graph::empty();
    measure("bulk apply", || graph.apply(patch))?;
    memory("working graph");
    let mut edited = graph.entity("n0").unwrap().clone();
    edited.label = "Edited record".into();
    measure("single entity edit", || {
        graph.apply(Patch {
            entities: vec![edited],
            ..Patch::default()
        })
    })?;
    let moved = (0..1000.min(count))
        .map(|index| {
            let mut entity = graph.entity(&format!("n{index}")).unwrap().clone();
            entity.x += 3.0;
            entity
        })
        .collect();
    measure("move 1000 entities", || {
        graph.apply(Patch {
            entities: moved,
            ..Patch::default()
        })
    })?;
    measure("delete 100 entities + incident edges", || {
        graph.apply(Patch {
            remove_entities: (0..100).map(|index| format!("n{index}")).collect(),
            ..Patch::default()
        })
    })?;
    measure("undo deletion", || graph.undo())?;
    let project = measure("portable snapshot", || graph.snapshot());
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("benchmark.linkscope");
    measure("first atomic save", || {
        storage::save(Path::new(&path), &project)
    })?;
    measure("replace save + backup", || {
        storage::save(Path::new(&path), &project)
    })?;
    println!("project bytes: {}", std::fs::metadata(&path)?.len());
    let restored = measure("load + validate", || storage::load(&path))?;
    let reopened = measure("build reopened working graph", || {
        Graph::new(restored.project)
    })?;
    assert_eq!(reopened.entity_count(), count);
    assert_eq!(reopened.relationship_count(), edges);
    memory("graph + portable snapshot + reopened graph");
    println!("Rendering is measured separately; these timings do not measure frame rate.");
    Ok(())
}
