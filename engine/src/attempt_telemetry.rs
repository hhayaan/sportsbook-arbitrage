use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

fn data_dir() -> PathBuf {
    std::env::var("ATTEMPT_TELEMETRY_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data"))
}

fn append_jsonl(path: &Path, record: &Value) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, record)?;
    file.write_all(b"\n")?;
    Ok(())
}

pub fn emit_event(event_type: &str, attempt_id: Option<&str>, payload: Value) -> io::Result<()> {
    let record = serde_json::json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "event_type": event_type,
        "attempt_id": attempt_id,
        "payload": payload,
    });

    let root = data_dir();
    append_jsonl(&root.join("events.jsonl"), &record)?;

    if let Some(id) = attempt_id {
        append_jsonl(&root.join("bets").join(format!("{id}.jsonl")), &record)?;
    }

    Ok(())
}
