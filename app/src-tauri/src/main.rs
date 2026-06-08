use flowhub_core::FlowHub;
use flowhub_discovery::PeerInfo;
use flowhub_send::{receive_file, send_file_with_progress, TransferControl};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;

const TRANSFER_PORT: u16 = 47322;

#[derive(Clone)]
struct TransferHandle {
    control: TransferControl,
    peer_ip: String,
    file_path: String,
    bytes_transferred: Arc<AtomicU64>,
}

type TransferRegistry = Arc<Mutex<HashMap<String, TransferHandle>>>;

struct AppState {
    app: Arc<Mutex<FlowHub>>,
    transfers: TransferRegistry,
}

#[derive(Clone, Serialize)]
struct TransferEvent {
    id: String,
    peer_ip: String,
    file_path: String,
    status: String,
    bytes_transferred: u64,
    total_bytes: u64,
    bytes_per_second: u64,
    eta_seconds: Option<u64>,
    error: Option<String>,
}

#[tauri::command]
async fn list_peers(state: State<'_, AppState>) -> Result<Vec<PeerInfo>, String> {
    let app = state.app.lock().await;
    Ok(app.list_peers())
}

#[tauri::command]
async fn download_tasks(
    state: State<'_, AppState>,
) -> Result<Vec<flowhub_core::DownloadTaskView>, String> {
    let app = state.app.lock().await;
    app.download_tasks()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn add_download(state: State<'_, AppState>, url: String) -> Result<String, String> {
    let app = state.app.lock().await;
    app.add_download(&url)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn pause_download(state: State<'_, AppState>, gid: String) -> Result<(), String> {
    let app = state.app.lock().await;
    app.pause_download(&gid)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn resume_download(state: State<'_, AppState>, gid: String) -> Result<(), String> {
    let app = state.app.lock().await;
    app.resume_download(&gid)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn remove_download(state: State<'_, AppState>, gid: String) -> Result<(), String> {
    let app = state.app.lock().await;
    app.remove_download(&gid)
        .await
        .map_err(|error| error.to_string())
}

#[derive(Clone, Serialize, Deserialize)]
struct AppSettings {
    aria2_endpoint: String,
    aria2_secret: String,
}

#[tauri::command]
async fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    let app = state.app.lock().await;
    let aria2_endpoint = app
        .get_setting("aria2_endpoint")
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "http://127.0.0.1:6800/jsonrpc".to_string());
    let aria2_secret = app
        .get_setting("aria2_secret")
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    Ok(AppSettings { aria2_endpoint, aria2_secret })
}

#[tauri::command]
async fn save_settings(state: State<'_, AppState>, settings: AppSettings) -> Result<(), String> {
    let app = state.app.lock().await;
    app.save_setting("aria2_endpoint", &settings.aria2_endpoint)
        .map_err(|e| e.to_string())?;
    app.save_setting("aria2_secret", &settings.aria2_secret)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Clone, Serialize)]
struct TransferHistoryItem {
    id: String,
    status: String,
    target: String,
}

#[tauri::command]
async fn list_transfer_history(
    state: State<'_, AppState>,
) -> Result<Vec<TransferHistoryItem>, String> {
    let app = state.app.lock().await;
    let tasks = app.list_transfer_tasks().map_err(|e| e.to_string())?;
    Ok(tasks
        .into_iter()
        .map(|t| TransferHistoryItem {
            id: t.id,
            status: t.status,
            target: t.target,
        })
        .collect())
}

#[tauri::command]
async fn send_file_to_peer(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    peer_ip: String,
    file_path: String,
) -> Result<(), String> {
    let transfer_id = format!("{peer_ip}:{file_path}");
    run_send_transfer(
        app_handle,
        state.transfers.clone(),
        state.app.clone(),
        transfer_id,
        peer_ip,
        file_path,
        0,
    )
    .await
}

#[tauri::command]
async fn pause_transfer(state: State<'_, AppState>, transfer_id: String) -> Result<(), String> {
    let transfers = state.transfers.lock().await;
    match transfers.get(&transfer_id) {
        Some(handle) => {
            handle.control.pause();
            Ok(())
        }
        None => Err(format!("unknown transfer {transfer_id}")),
    }
}

#[tauri::command]
async fn resume_transfer(state: State<'_, AppState>, transfer_id: String) -> Result<(), String> {
    let transfers = state.transfers.lock().await;
    match transfers.get(&transfer_id) {
        Some(handle) => {
            handle.control.resume();
            Ok(())
        }
        None => Err(format!("unknown transfer {transfer_id}")),
    }
}

#[tauri::command]
async fn retry_transfer(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    transfer_id: String,
) -> Result<(), String> {
    let (peer_ip, file_path, resume_from) = {
        let transfers = state.transfers.lock().await;
        match transfers.get(&transfer_id) {
            Some(handle) => (
                handle.peer_ip.clone(),
                handle.file_path.clone(),
                handle.bytes_transferred.load(Ordering::SeqCst),
            ),
            None => return Err(format!("unknown transfer {transfer_id}")),
        }
    };

    run_send_transfer(
        app_handle,
        state.transfers.clone(),
        state.app.clone(),
        transfer_id,
        peer_ip,
        file_path,
        resume_from,
    )
    .await
}

async fn run_send_transfer(
    app_handle: AppHandle,
    transfers: TransferRegistry,
    app: Arc<Mutex<FlowHub>>,
    transfer_id: String,
    peer_ip: String,
    file_path: String,
    resume_from: u64,
) -> Result<(), String> {
    let target_addr = transfer_addr(&peer_ip);
    let control = TransferControl::default();
    let bytes_transferred = Arc::new(AtomicU64::new(resume_from));

    transfers.lock().await.insert(
        transfer_id.clone(),
        TransferHandle {
            control: control.clone(),
            peer_ip: peer_ip.clone(),
            file_path: file_path.clone(),
            bytes_transferred: bytes_transferred.clone(),
        },
    );

    {
        let flowhub = app.lock().await;
        let _ = flowhub.upsert_transfer_task(&transfer_id, "sending", &file_path);
    }

    let event_base = TransferEvent {
        id: transfer_id.clone(),
        peer_ip: peer_ip.clone(),
        file_path: file_path.clone(),
        status: "sending".into(),
        bytes_transferred: resume_from,
        total_bytes: 0,
        bytes_per_second: 0,
        eta_seconds: None,
        error: None,
    };
    app_handle
        .emit("transfer-progress", event_base.clone())
        .map_err(|error| error.to_string())?;

    let progress_handle = app_handle.clone();
    let progress_base = event_base.clone();
    let progress_bytes = bytes_transferred.clone();
    let result = send_file_with_progress(
        &target_addr,
        &file_path,
        resume_from,
        control,
        move |progress| {
            progress_bytes.store(progress.bytes_transferred, Ordering::SeqCst);
            let _ = progress_handle.emit(
                "transfer-progress",
                TransferEvent {
                    status: "sending".into(),
                    bytes_transferred: progress.bytes_transferred,
                    total_bytes: progress.total_bytes,
                    bytes_per_second: progress.bytes_per_second,
                    eta_seconds: progress.eta_seconds,
                    ..progress_base.clone()
                },
            );
        },
    )
    .await;

    match result {
        Ok(progress) => {
            bytes_transferred.store(progress.bytes_transferred, Ordering::SeqCst);
            {
                let flowhub = app.lock().await;
                let _ = flowhub.finish_transfer_task(&transfer_id, "completed");
            }
            app_handle
                .emit(
                    "transfer-progress",
                    TransferEvent {
                        status: "completed".into(),
                        bytes_transferred: progress.bytes_transferred,
                        total_bytes: progress.total_bytes,
                        bytes_per_second: progress.bytes_per_second,
                        eta_seconds: Some(0),
                        ..event_base
                    },
                )
                .map_err(|error| error.to_string())?;
            Ok(())
        }
        Err(error) => {
            let message = error.to_string();
            {
                let flowhub = app.lock().await;
                let _ = flowhub.finish_transfer_task(&transfer_id, "error");
            }
            app_handle
                .emit(
                    "transfer-progress",
                    TransferEvent {
                        status: "error".into(),
                        error: Some(message.clone()),
                        bytes_transferred: bytes_transferred.load(Ordering::SeqCst),
                        ..event_base
                    },
                )
                .map_err(|emit_error| emit_error.to_string())?;
            Err(message)
        }
    }
}

fn main() {
    let app = FlowHub::new_default().expect("failed to initialize FlowHub");
    let discovery = app.discovery_service();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            app: Arc::new(Mutex::new(app)),
            transfers: Arc::new(Mutex::new(HashMap::new())),
        })
        .setup(move |_| {
            tauri::async_runtime::spawn(async move {
                if let Err(error) = discovery.run().await {
                    eprintln!("discovery service stopped: {error}");
                }
            });

            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                receive_loop(app_handle).await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_peers,
            download_tasks,
            add_download,
            pause_download,
            resume_download,
            remove_download,
            send_file_to_peer,
            pause_transfer,
            resume_transfer,
            retry_transfer,
            list_transfer_history,
            get_settings,
            save_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running FlowHub");
}

async fn receive_loop(app_handle: AppHandle) {
    let output_dir = std::env::current_dir()
        .unwrap_or_else(|_| ".".into())
        .join("flowhub-received");
    let bind_addr = format!("0.0.0.0:{TRANSFER_PORT}");

    loop {
        match receive_file(&bind_addr, &output_dir, TransferControl::default()).await {
            Ok(progress) => {
                let _ = app_handle.emit(
                    "transfer-received",
                    TransferEvent {
                        id: format!("received:{}", progress.sha256.clone().unwrap_or_default()),
                        peer_ip: "incoming".into(),
                        file_path: output_dir.to_string_lossy().to_string(),
                        status: "received".into(),
                        bytes_transferred: progress.bytes_transferred,
                        total_bytes: progress.total_bytes,
                        bytes_per_second: progress.bytes_per_second,
                        eta_seconds: progress.eta_seconds,
                        error: None,
                    },
                );
            }
            Err(error) => {
                let _ = app_handle.emit(
                    "transfer-received",
                    TransferEvent {
                        id: "receiver".into(),
                        peer_ip: "incoming".into(),
                        file_path: output_dir.to_string_lossy().to_string(),
                        status: "error".into(),
                        bytes_transferred: 0,
                        total_bytes: 0,
                        bytes_per_second: 0,
                        eta_seconds: None,
                        error: Some(error.to_string()),
                    },
                );
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
}

fn transfer_addr(peer_ip: &str) -> String {
    if peer_ip.contains(':') && !peer_ip.starts_with('[') {
        format!("[{peer_ip}]:{TRANSFER_PORT}")
    } else {
        format!("{peer_ip}:{TRANSFER_PORT}")
    }
}
