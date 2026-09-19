use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{watch, RwLock};

use crate::config::Config;
use crate::db::Db;
use crate::docker::{AppJob, ImageUpdate};
use crate::stats::{SummaryRx, SummaryTx};

pub type AppJobsTx = watch::Sender<HashMap<String, AppJob>>;
pub type ImageUpdates = Arc<RwLock<HashMap<String, ImageUpdate>>>;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub config_path: PathBuf,
    pub db: Arc<Db>,
    pub summary_tx: SummaryTx,
    pub summary_rx: SummaryRx,
    pub jobs_tx: AppJobsTx,
    pub image_updates: ImageUpdates,
    pub http: reqwest::Client,
}

impl AppState {
    pub fn job(&self, id: &str) -> Option<AppJob> {
        self.jobs_tx.borrow().get(id).cloned()
    }

    pub fn set_job(&self, job: AppJob) {
        self.jobs_tx.send_modify(|m| {
            m.insert(job.id.clone(), job);
        });
    }

    pub fn clear_job(&self, id: &str) {
        self.jobs_tx.send_modify(|m| {
            m.remove(id);
        });
    }
}
