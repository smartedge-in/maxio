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
        "message": "Admin API not implemented yet — wire P2-13 server endpoints and replace this stub response."
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
        other => other.to_string(),
    }
}