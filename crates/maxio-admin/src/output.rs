use crate::error::Result;
use serde_json::Value;

pub fn emit(json_mode: bool, value: &Value) -> Result<()> {
    if json_mode {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        print_human(value);
    }
    Ok(())
}

pub fn emit_stub(json_mode: bool, command: &str, endpoint: &str, profile: &str) -> Result<()> {
    let value = serde_json::json!({
        "status": "stub",
        "command": command,
        "profile": profile,
        "endpoint": endpoint,
        "message": "Admin API not available on the target server."
    });
    emit(json_mode, &value)
}

pub fn emit_message(json_mode: bool, message: &str) {
    if json_mode {
        println!(
            "{}",
            serde_json::json!({ "message": message }).to_string()
        );
    } else {
        println!("{message}");
    }
}

fn print_human(value: &Value) {
    if let Some(checks) = value.get("checks").and_then(|c| c.as_array()) {
        let ok = value.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
        println!("Overall: {}", if ok { "OK" } else { "FAILED" });
        for check in checks {
            let name = check.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let status = if check.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                "ok"
            } else {
                "FAIL"
            };
            let detail = check.get("detail").and_then(|v| v.as_str()).unwrap_or("");
            println!("  [{status}] {name}: {detail}");
        }
        return;
    }

    if let Some(buckets) = value.get("buckets").and_then(|b| b.as_array()) {
        println!("{:<24}  {:>8}  {}", "BUCKET", "OBJECTS", "CREATED");
        for bucket in buckets {
            println!(
                "{:<24}  {:>8}  {}",
                bucket.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                bucket
                    .get("object_count")
                    .map(format_value)
                    .unwrap_or_else(|| "?".into()),
                bucket
                    .get("created_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
            );
        }
        return;
    }

    if let Some(keys) = value.get("keys").and_then(|k| k.as_array()) {
        println!("Active key: {}", value.get("active_id").and_then(|v| v.as_str()).unwrap_or("?"));
        println!("{:<20}  {:<26}  ACTIVE", "KEY_ID", "CREATED_AT");
        for key in keys {
            println!(
                "{:<20}  {:<26}  {}",
                key.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
                key.get("created_at").and_then(|v| v.as_str()).unwrap_or("?"),
                if key.get("active").and_then(|v| v.as_bool()).unwrap_or(false) {
                    "yes"
                } else {
                    "no"
                }
            );
        }
        return;
    }

    match value {
        Value::Object(map) => {
            for (k, v) in map {
                println!("{k}: {}", format_value(v));
            }
        }
        other => println!("{}", format_value(other)),
    }
}

fn format_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Object(_) | Value::Array(_) => value.to_string(),
    }
}