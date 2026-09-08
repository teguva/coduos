use serde::Serialize;
use tokio::process::Command;

use crate::config::UnitSpec;
use crate::error::ApiError;

#[derive(Debug, Clone, Serialize)]
pub struct UnitStatus {
    pub id: String,
    pub unit: String,
    pub label: String,
    pub active: String,
    pub enabled: String,
    pub description: String,
}

fn validate_unit_name(unit: &str) -> Result<(), ApiError> {
    if unit.is_empty()
        || unit.len() > 128
        || !unit
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@' | ':'))
        || unit.contains('/')
        || unit.contains("..")
    {
        return Err(ApiError::BadRequest("invalid unit name".into()));
    }
    Ok(())
}

async fn run(args: &[&str]) -> Result<(bool, String, String), ApiError> {
    let out = Command::new("systemctl")
        .args(args)
        .output()
        .await
        .map_err(|err| ApiError::BadRequest(format!("systemctl: {err}")))?;
    Ok((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
        String::from_utf8_lossy(&out.stderr).trim().to_string(),
    ))
}

pub async fn status(spec: &UnitSpec) -> Result<UnitStatus, ApiError> {
    validate_unit_name(&spec.unit)?;
    let (_, active, _) = run(&["is-active", &spec.unit]).await?;
    let (_, enabled, _) = run(&["is-enabled", &spec.unit]).await?;
    let (_, show, _) = run(&["show", &spec.unit, "--property=Description", "--no-page"]).await?;
    let description = show
        .strip_prefix("Description=")
        .unwrap_or(&show)
        .to_string();
    Ok(UnitStatus {
        id: spec.id.clone(),
        unit: spec.unit.clone(),
        label: spec.label.clone(),
        active: if active.is_empty() {
            "unknown".into()
        } else {
            active
        },
        enabled: if enabled.is_empty() {
            "unknown".into()
        } else {
            enabled
        },
        description,
    })
}

pub async fn action(spec: &UnitSpec, action: &str) -> Result<(), ApiError> {
    validate_unit_name(&spec.unit)?;
    let verb = match action {
        "start" | "stop" | "restart" | "enable" | "disable" => action,
        _ => return Err(ApiError::BadRequest("unknown unit action".into())),
    };
    let (ok, _, err) = run(&[verb, &spec.unit]).await?;
    if !ok {
        return Err(ApiError::BadRequest(if err.is_empty() {
            format!("systemctl {verb} failed")
        } else {
            err
        }));
    }
    Ok(())
}

pub async fn journal(spec: &UnitSpec, lines: u32) -> Result<String, ApiError> {
    validate_unit_name(&spec.unit)?;
    let n = lines.clamp(1, 500).to_string();
    let out = Command::new("journalctl")
        .args(["-u", &spec.unit, "-n", &n, "--no-pager", "-o", "cat"])
        .output()
        .await
        .map_err(|err| ApiError::BadRequest(format!("journalctl: {err}")))?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}
