pub mod agent;
pub mod commits;
pub mod decisions;
pub mod session_work;
pub mod history;
pub mod presentation;
pub mod reasoning;
pub mod repository;
pub mod security;
pub mod service;
pub mod source;
pub mod insights;
pub mod storage;
pub mod tui;

use anyhow::{Result, bail};
use serde_json::Value;

pub fn s(v: &Value) -> &str { v.as_str().unwrap_or("") }
pub fn arr(v: &Value) -> &[Value] { v.as_array().map(Vec::as_slice).unwrap_or(&[]) }
pub fn n(v: &Value) -> usize { v.as_u64().unwrap_or(0) as usize }
pub fn now() -> String { chrono::Utc::now().to_rfc3339() }
pub fn id(prefix: &str) -> String { format!("{prefix}-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]) }
pub fn schema(name: &str) -> Value {
    let schemas: Value = serde_json::from_str(include_str!("data/schemas.json")).expect("embedded schemas");
    schemas[name].clone()
}
pub fn validate(name: &str, data: &Value) -> Result<()> {
    let schema = schema(name);
    let validator = jsonschema::validator_for(&schema)?;
    if !validator.is_valid(data) { bail!("Invalid input or structured response ({name})"); }
    Ok(())
}
