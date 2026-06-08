use anyhow::Context;
use flowhub_discovery::{DiscoveryService, PeerInfo, DEFAULT_DISCOVERY_PORT};
use flowhub_download::{Aria2Client, DownloadStatus};
use flowhub_storage::{Storage, TaskMetadata};
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

const DEFAULT_ARIA2_ENDPOINT: &str = "http://127.0.0.1:6800/jsonrpc";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadTaskView {
    pub id: String,
    pub status: String,
    pub target: String,
    pub progress: u64,
    pub total_bytes: u64,
    pub completed_bytes: u64,
    pub download_speed: u64,
    pub eta_seconds: Option<u64>,
}

pub struct FlowHub {
    discovery: DiscoveryService,
    downloads: Aria2Client,
    storage: Storage,
}

impl FlowHub {
    pub fn new_default() -> anyhow::Result<Self> {
        let storage = Storage::open("flowhub.db")?;

        let device_id = match storage.get_setting("device_id")? {
            Some(id) => id,
            None => {
                let id = Uuid::new_v4().to_string();
                storage.set_setting("device_id", &id)?;
                id
            }
        };

        let aria2_endpoint = storage
            .get_setting("aria2_endpoint")?
            .unwrap_or_else(|| DEFAULT_ARIA2_ENDPOINT.to_string());
        let aria2_secret = storage.get_setting("aria2_secret")?;

        Self::new(
            DiscoveryService::with_device_id(device_id, DEFAULT_DISCOVERY_PORT)?,
            Aria2Client::new(&aria2_endpoint, aria2_secret),
            storage,
        )
    }

    pub fn new(
        discovery: DiscoveryService,
        downloads: Aria2Client,
        storage: Storage,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            discovery,
            downloads,
            storage,
        })
    }

    pub fn with_storage_path(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        Self::new(
            DiscoveryService::new(DEFAULT_DISCOVERY_PORT)?,
            Aria2Client::new("http://127.0.0.1:6800/jsonrpc", None),
            Storage::open(path)?,
        )
    }

    pub fn list_peers(&self) -> Vec<PeerInfo> {
        self.discovery.list_peers()
    }

    pub fn discovery_service(&self) -> DiscoveryService {
        self.discovery.clone()
    }

    pub async fn add_download(&self, url: &str) -> anyhow::Result<String> {
        match self.downloads.add_url(url).await {
            Ok(gid) => {
                self.storage.upsert_task(&TaskMetadata {
                    id: gid.clone(),
                    kind: "download".into(),
                    status: "active".into(),
                    target: url.to_string(),
                    progress: 0,
                })?;
                Ok(gid)
            }
            Err(error) => {
                let task_id = Uuid::new_v4().to_string();
                self.storage.upsert_task(&TaskMetadata {
                    id: task_id.clone(),
                    kind: "download".into(),
                    status: "error".into(),
                    target: url.to_string(),
                    progress: 0,
                })?;
                self.storage.update_task_status(&task_id, "error")?;
                Err(error).context("failed to add download through aria2")
            }
        }
    }

    pub async fn pause_download(&self, gid: &str) -> anyhow::Result<()> {
        self.downloads.pause(gid).await?;
        self.storage.update_task_status(gid, "paused")?;
        Ok(())
    }

    pub async fn resume_download(&self, gid: &str) -> anyhow::Result<()> {
        self.downloads.resume(gid).await?;
        self.storage.update_task_status(gid, "active")?;
        Ok(())
    }

    pub async fn remove_download(&self, gid: &str) -> anyhow::Result<()> {
        if let Err(error) = self.downloads.remove(gid).await {
            let message = error.to_string();
            if !message.contains("not found") && !message.contains("is not found") {
                return Err(error).context("failed to remove download through aria2");
            }
        }
        self.storage.remove_task(gid)?;
        Ok(())
    }

    pub fn upsert_transfer_task(&self, id: &str, status: &str, target: &str) -> anyhow::Result<()> {
        self.storage.upsert_task(&TaskMetadata {
            id: id.to_string(),
            kind: "send".into(),
            status: status.to_string(),
            target: target.to_string(),
            progress: 0,
        })
    }

    pub fn finish_transfer_task(&self, id: &str, status: &str) -> anyhow::Result<()> {
        let progress = if status == "completed" { 100 } else { 0 };
        self.storage.update_task_progress(id, status, progress)
    }

    pub fn list_transfer_tasks(&self) -> anyhow::Result<Vec<TaskMetadata>> {
        Ok(self
            .storage
            .list_tasks()?
            .into_iter()
            .filter(|t| t.kind == "send")
            .collect())
    }

    pub fn get_setting(&self, key: &str) -> anyhow::Result<Option<String>> {
        self.storage.get_setting(key)
    }

    pub fn save_setting(&self, key: &str, value: &str) -> anyhow::Result<()> {
        self.storage.set_setting(key, value)
    }

    pub async fn download_tasks(&self) -> anyhow::Result<Vec<DownloadTaskView>> {
        let tasks = self
            .storage
            .list_tasks()?
            .into_iter()
            .filter(|task| task.kind == "download")
            .collect::<Vec<_>>();

        let mut views = Vec::with_capacity(tasks.len());
        for task in tasks {
            match self.downloads.status(&task.id).await {
                Ok(status) => {
                    let view = task_view_from_status(&task, status);
                    self.storage
                        .update_task_progress(&task.id, &view.status, view.progress)?;
                    views.push(view);
                }
                Err(_) => views.push(stored_task_view(task)),
            }
        }

        Ok(views)
    }
}

fn task_view_from_status(task: &TaskMetadata, status: DownloadStatus) -> DownloadTaskView {
    let progress = if status.total_length == 0 {
        task.progress
    } else {
        status
            .completed_length
            .saturating_mul(100)
            .checked_div(status.total_length)
            .unwrap_or(0)
            .min(100)
    };
    let eta_seconds = if status.download_speed == 0 {
        None
    } else {
        Some(
            status
                .total_length
                .saturating_sub(status.completed_length)
                .checked_div(status.download_speed)
                .unwrap_or(0),
        )
    };
    let target = status
        .files
        .first()
        .filter(|path| !path.is_empty())
        .cloned()
        .unwrap_or_else(|| task.target.clone());

    DownloadTaskView {
        id: task.id.clone(),
        status: status.status,
        target,
        progress,
        total_bytes: status.total_length,
        completed_bytes: status.completed_length,
        download_speed: status.download_speed,
        eta_seconds,
    }
}

fn stored_task_view(task: TaskMetadata) -> DownloadTaskView {
    DownloadTaskView {
        id: task.id,
        status: task.status,
        target: task.target,
        progress: task.progress,
        total_bytes: 0,
        completed_bytes: 0,
        download_speed: 0,
        eta_seconds: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowhub_discovery::DiscoveryMessage;
    use flowhub_storage::Storage;

    #[test]
    fn exposes_peers_from_discovery() {
        let discovery = DiscoveryService::with_local(
            DiscoveryMessage {
                device_id: "local".into(),
                hostname: "local".into(),
                version: "0.1.0".into(),
            },
            0,
        );
        discovery.observe_message(
            DiscoveryMessage {
                device_id: "remote".into(),
                hostname: "remote".into(),
                version: "0.1.0".into(),
            },
            "127.0.0.1:1".parse().unwrap(),
        );

        let app = FlowHub::new(
            discovery,
            Aria2Client::new("http://127.0.0.1:6800/jsonrpc", None),
            Storage::in_memory().unwrap(),
        )
        .unwrap();

        assert_eq!(app.list_peers().len(), 1);
    }
}
