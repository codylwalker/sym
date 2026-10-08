//! An OpenAI-shaped embeddings client over plain HTTP (loopback or the
//! mesh): `POST {base}/v1/embeddings {"model","input":[…]}` →
//! `data[].embedding`. No TLS by default: the estate's embedder is local.

use serde_json::{json, Value};

/// Normalize a base URL: accept `http://h:p`, `http://h:p/v1`, or the full
/// `/v1/embeddings` path.
pub fn endpoint(base: &str) -> String {
    let b = base.trim_end_matches('/');
    if b.ends_with("/embeddings") {
        b.to_string()
    } else if b.ends_with("/v1") {
        format!("{b}/embeddings")
    } else {
        format!("{b}/v1/embeddings")
    }
}

pub fn embed(base: &str, model: &str, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let body = json!({ "model": model, "input": texts });
    let mut resp = ureq::post(&endpoint(base))
        .header("content-type", "application/json")
        .send_json(&body)
        .map_err(|e| format!("embeddings request failed: {e}"))?;
    let v: Value = resp
        .body_mut()
        .read_json()
        .map_err(|e| format!("embeddings response unreadable: {e}"))?;
    let data = v
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("embeddings response has no data: {}", v.to_string().chars().take(200).collect::<String>()))?;
    let mut out = Vec::with_capacity(data.len());
    for d in data {
        let e = d
            .get("embedding")
            .and_then(Value::as_array)
            .ok_or("embedding missing")?;
        let mut vec: Vec<f32> = e.iter().filter_map(Value::as_f64).map(|x| x as f32).collect();
        normalize(&mut vec);
        out.push(vec);
    }
    if out.len() != texts.len() {
        return Err(format!("embeddings: sent {} texts, got {}", texts.len(), out.len()));
    }
    Ok(out)
}

pub fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        for x in v.iter_mut() {
            *x /= n;
        }
    }
}
