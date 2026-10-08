//! gray-structured — a `structured` tool that turns a task + JSON schema
//! into validated JSON via a nested `host/run` turn.
//!
//! Port of pi's `structured-output` extension. pi's version was a terminal
//! tool that ended the turn with a fixed-shape answer; gray's tool instead
//! asks the host to run a constrained sub-turn (capability `host.turn`,
//! ≤28s on the host, capped at 25s here) with
//! "Respond with ONLY a JSON object matching this JSON schema…", extracts the
//! first `{…}` block from the reply, and returns it pretty-printed. A reply
//! that yields no parseable JSON comes back `is_error` with the raw text.
//!
//! Blocking std I/O with a reader thread, same shape as gray-questions:
//! sidecar→host requests use string ids ("q1", …) and their replies are
//! routed back through a pending map.
//!
//! Wire methods: plugin/manifest, tool/call, plugin/shutdown.
//! Sidecar→host: host/run (capability host.turn).

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use serde_json::{Value, json};

/// Must sit under the host's own 28s host/run cap so the tool reports its
/// own timeout instead of hanging past it.
const RUN_TTL: Duration = Duration::from_secs(25);

type Pending = Arc<Mutex<HashMap<String, mpsc::Sender<Value>>>>;

fn manifest() -> Value {
    json!({
        "name": "structured",
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": "1.1",
        "capabilities": ["host.turn"],
        "tools": [{
            "name": "structured",
            "description": "Produce structured JSON for a task: runs a constrained sub-turn that must reply with only a JSON object matching your `schema`, then returns that JSON pretty-printed. Use when you need machine-readable output, extraction, or a decision in a fixed shape.",
            "parameters": {
                "type": "object",
                "properties": {
                    "prompt": { "type": "string", "description": "The task/question the sub-turn should answer." },
                    "schema": { "description": "JSON schema the reply must match — an object, or a raw schema string." }
                },
                "required": ["prompt", "schema"]
            }
        }],
        "commands": [],
    })
}

fn next_id(counter: &Arc<Mutex<u64>>) -> String {
    let mut n = counter.lock().expect("id counter");
    *n += 1;
    format!("q{n}")
}

/// "Respond with ONLY a JSON object matching this JSON schema…" per spec.
fn build_prompt(prompt: &str, schema: &Value) -> String {
    let schema_text = match schema {
        Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
    };
    format!(
        "Respond with ONLY a JSON object matching this JSON schema. Schema: {schema_text}\nTask: {prompt}"
    )
}

/// The first balanced `{…}` block in `text`, skipping braces inside strings.
fn extract_json_block(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let start = bytes.iter().position(|b| *b == b'{')?;
    let mut depth = 0usize;
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Extract + parse + pretty-print. Err carries the raw reply for is_error.
fn parse_structured(reply_text: &str) -> Result<String, String> {
    let Some(block) = extract_json_block(reply_text) else {
        return Err(format!(
            "structured: reply contained no JSON object. Raw reply:\n{reply_text}"
        ));
    };
    match serde_json::from_str::<Value>(block) {
        Ok(v) => Ok(serde_json::to_string_pretty(&v).unwrap_or_else(|_| block.to_string())),
        Err(e) => Err(format!(
            "structured: reply's first JSON block didn't parse ({e}). Raw reply:\n{reply_text}"
        )),
    }
}

/// The `structured` tool: host/run the constrained prompt, parse the reply.
fn structured_call(
    out: &Arc<Mutex<std::io::Stdout>>,
    pending: &Pending,
    counter: &Arc<Mutex<u64>>,
    args: &Value,
) -> Result<String, String> {
    let prompt = args
        .get("prompt")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or("missing required argument: prompt")?;
    let schema = args
        .get("schema")
        .filter(|s| !s.is_null())
        .ok_or("missing required argument: schema")?;
    let id = next_id(counter);
    let (tx, rx) = mpsc::channel();
    pending.lock().expect("pending").insert(id.clone(), tx);
    let req = json!({
        "id": id,
        "method": "host/run",
        "params": { "prompt": build_prompt(prompt, schema) },
    });
    {
        let mut o = out.lock().expect("stdout");
        let _ = writeln!(o, "{req}");
        let _ = o.flush();
    }
    // recv_timeout also fails fast when the reader thread dies on shutdown.
    let reply = rx
        .recv_timeout(RUN_TTL)
        .map_err(|_| "structured: host/run did not answer within 25s (host.turn granted?)".to_string());
    pending.lock().expect("pending").remove(&id);
    let reply = reply?;
    if let Some(e) = reply.get("error") {
        return Err(format!("structured: host/run failed: {e}"));
    }
    let text = reply
        .get("result")
        .and_then(|r| r.get("text"))
        .and_then(Value::as_str)
        .ok_or("structured: host/run reply had no result.text")?;
    parse_structured(text)
}

fn main() -> std::io::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("manifest") {
        println!("{}", manifest());
        return Ok(());
    }
    let stdout = Arc::new(Mutex::new(std::io::stdout()));
    let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
    let counter = Arc::new(Mutex::new(0u64));

    // Reader thread: sidecar→host replies carry our string id and no method
    // → route to the pending map; everything else queues for the main loop.
    let (work_tx, work_rx) = mpsc::channel::<Value>();
    let reader_pending = pending.clone();
    let reader = std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            if v.get("method").and_then(|m| m.as_str()) == Some("plugin/shutdown") {
                break;
            }
            if let Some(id) = v.get("id").and_then(|i| i.as_str())
                && v.get("method").is_none()
                && let Some(tx) = reader_pending.lock().expect("pending").remove(id)
            {
                let _ = tx.send(v);
                continue;
            }
            if v.get("id").and_then(|i| i.as_u64()).is_none() {
                continue; // notification
            }
            if work_tx.send(v).is_err() {
                break;
            }
        }
    });

    for req in work_rx {
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let id = req.get("id").cloned().unwrap_or(json!(0));
        let params = req.get("params").cloned().unwrap_or(Value::Null);
        let result = match method {
            "plugin/manifest" => manifest(),
            "tool/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("args").cloned().unwrap_or(Value::Null);
                match if name == "structured" {
                    structured_call(&stdout, &pending, &counter, &args)
                } else {
                    Err(format!("unknown tool: {name}"))
                } {
                    Ok(text) => json!({ "content": text }),
                    Err(text) => json!({ "content": text, "is_error": true }),
                }
            }
            "plugin/shutdown" => {
                let reply = json!({ "id": id, "result": {} });
                let mut o = stdout.lock().expect("stdout");
                let _ = writeln!(o, "{reply}");
                let _ = o.flush();
                break;
            }
            _ => continue,
        };
        let reply = json!({ "id": id, "result": result });
        let mut o = stdout.lock().expect("stdout");
        let _ = writeln!(o, "{reply}");
        let _ = o.flush();
    }
    let _ = reader.join();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_declares_tool_and_capability() {
        let m = manifest();
        assert_eq!(m["name"], "structured");
        assert_eq!(m["capabilities"], json!(["host.turn"]));
        assert_eq!(m["tools"][0]["name"], "structured");
        let req = m["tools"][0]["parameters"]["required"].as_array().unwrap();
        assert!(req.contains(&json!("prompt")) && req.contains(&json!("schema")));
    }

    #[test]
    fn prompt_wraps_schema_and_task() {
        let p = build_prompt("summarize", &json!({"type": "object"}));
        assert!(p.contains("Respond with ONLY a JSON object matching this JSON schema."));
        assert!(p.contains("\"type\": \"object\""));
        assert!(p.ends_with("Task: summarize"));
        // String schemas pass through raw.
        let p = build_prompt("x", &json!("{\"a\":1}"));
        assert!(p.contains("Schema: {\"a\":1}"));
    }

    #[test]
    fn extract_finds_first_balanced_block() {
        let t = "Sure! {\"a\": {\"b\": \"} not end\"}} trailing {\"ignored\":1}";
        assert_eq!(extract_json_block(t).unwrap(), "{\"a\": {\"b\": \"} not end\"}}");
        assert!(extract_json_block("no json here").is_none());
        assert!(extract_json_block("{\"unclosed\":").is_none());
    }

    #[test]
    fn parse_pretty_prints_the_block() {
        let out = parse_structured("noise {\"a\":1} tail").unwrap();
        assert_eq!(out, "{\n  \"a\": 1\n}");
    }

    #[test]
    fn parse_errors_carry_raw_text() {
        let e = parse_structured("I cannot do that").unwrap_err();
        assert!(e.contains("I cannot do that"));
        let e = parse_structured("prefix {bad json} suffix").unwrap_err();
        assert!(e.contains("didn't parse") && e.contains("{bad json}"));
    }

    #[test]
    fn missing_args_are_errors() {
        let out = Arc::new(Mutex::new(std::io::stdout()));
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let counter = Arc::new(Mutex::new(0u64));
        let e = structured_call(&out, &pending, &counter, &json!({})).unwrap_err();
        assert!(e.contains("prompt"));
        let e = structured_call(&out, &pending, &counter, &json!({"prompt": "p"})).unwrap_err();
        assert!(e.contains("schema"));
    }
}
