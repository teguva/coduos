use serde::{Deserialize, Serialize};

use crate::error::ApiError;

#[derive(Debug, Serialize)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: Option<String>,
    pub html_url: Option<String>,
    pub up_to_date: bool,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
}

pub async fn check(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
) -> Result<UpdateInfo, ApiError> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let url = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");
    let res = client
        .get(&url)
        .header("user-agent", format!("CoduOS/{current}"))
        .header("accept", "application/vnd.github+json")
        .send()
        .await;
    match res {
        Ok(resp) if resp.status().is_success() => {
            let body = resp.json::<GhRelease>().await.map_err(ApiError::internal)?;
            let latest = body.tag_name.trim_start_matches('v').to_string();
            Ok(UpdateInfo {
                up_to_date: latest == current || body.tag_name == format!("v{current}"),
                latest: Some(latest),
                html_url: Some(body.html_url),
                current,
                error: None,
            })
        }
        Ok(resp) => Ok(UpdateInfo {
            current,
            latest: None,
            html_url: None,
            up_to_date: true,
            error: Some(format!("GitHub HTTP {}", resp.status())),
        }),
        Err(err) => Ok(UpdateInfo {
            current,
            latest: None,
            html_url: None,
            up_to_date: true,
            error: Some(err.to_string()),
        }),
    }
}
