use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::config::Config;
use crate::db::Db;
use crate::stats::{SummaryRx, SummaryTx};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub config_path: PathBuf,
    pub db: Arc<Db>,
    pub summary_tx: SummaryTx,
    pub summary_rx: SummaryRx,
    pub http: reqwest::Client,
}
