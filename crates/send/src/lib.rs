use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProgress {
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub bytes_per_second: u64,
    pub eta_seconds: Option<u64>,
    pub sha256: Option<String>,
}

#[derive(Clone, Default)]
pub struct TransferControl {
    paused: Arc<AtomicBool>,
}

impl TransferControl {
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }
}

pub fn pause(control: &TransferControl) {
    control.pause();
}

pub fn resume(control: &TransferControl) {
    control.resume();
}

pub async fn send_file(
    target_addr: &str,
    path: impl AsRef<Path>,
    resume_from: u64,
    control: TransferControl,
) -> anyhow::Result<TransferProgress> {
    send_file_with_progress(target_addr, path, resume_from, control, |_| {}).await
}

pub async fn send_file_with_progress(
    target_addr: &str,
    path: impl AsRef<Path>,
    resume_from: u64,
    control: TransferControl,
    mut on_progress: impl FnMut(TransferProgress) + Send,
) -> anyhow::Result<TransferProgress> {
    let path = path.as_ref();
    let metadata = fs::metadata(path).await?;
    let mut file = fs::File::open(path).await?;
    file.seek(std::io::SeekFrom::Start(resume_from)).await?;

    let mut stream = TcpStream::connect(target_addr).await?;
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("file path has no valid file name"))?;
    let sha256 = sha256_file(path).await?;
    let header = TransferHeader {
        filename: filename.to_string(),
        total_bytes: metadata.len(),
        resume_from,
        sha256: sha256.clone(),
    };
    write_header(&mut stream, &header).await?;

    let mut transferred = resume_from;
    let mut buf = vec![0_u8; 64 * 1024];
    let started_at = std::time::Instant::now();
    loop {
        while control.is_paused() {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        let read = file.read(&mut buf).await?;
        if read == 0 {
            break;
        }
        stream.write_all(&buf[..read]).await?;
        transferred += read as u64;

        on_progress(progress_snapshot(
            transferred,
            metadata.len(),
            started_at,
            Some(sha256.clone()),
        ));
    }

    Ok(TransferProgress {
        bytes_transferred: transferred,
        total_bytes: metadata.len(),
        bytes_per_second: progress_speed(transferred.saturating_sub(resume_from), started_at),
        eta_seconds: Some(0),
        sha256: Some(sha256),
    })
}

pub async fn receive_file(
    bind_addr: &str,
    output_dir: impl AsRef<Path>,
    control: TransferControl,
) -> anyhow::Result<TransferProgress> {
    let listener = TcpListener::bind(bind_addr).await?;
    let (mut stream, _) = listener.accept().await?;
    let header = read_header(&mut stream).await?;
    let output_path = safe_output_path(output_dir.as_ref(), &header.filename);
    fs::create_dir_all(output_dir.as_ref()).await?;

    let current_len = fs::metadata(&output_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    let start_at = current_len.max(header.resume_from).min(header.total_bytes);
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .open(&output_path)
        .await?;
    file.seek(std::io::SeekFrom::Start(start_at)).await?;

    let mut transferred = start_at;
    let mut buf = vec![0_u8; 64 * 1024];
    while transferred < header.total_bytes {
        while control.is_paused() {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        let read = stream.read(&mut buf).await?;
        if read == 0 {
            break;
        }
        file.write_all(&buf[..read]).await?;
        transferred += read as u64;
    }
    file.flush().await?;

    let actual = sha256_file(&output_path).await?;
    if transferred == header.total_bytes && actual != header.sha256 {
        return Err(anyhow!(
            "sha256 mismatch after receiving {}",
            header.filename
        ));
    }

    Ok(TransferProgress {
        bytes_transferred: transferred,
        total_bytes: header.total_bytes,
        bytes_per_second: 0,
        eta_seconds: Some(0),
        sha256: Some(actual),
    })
}

fn progress_snapshot(
    bytes_transferred: u64,
    total_bytes: u64,
    started_at: std::time::Instant,
    sha256: Option<String>,
) -> TransferProgress {
    let bytes_per_second = progress_speed(bytes_transferred, started_at);
    let eta_seconds = if bytes_per_second == 0 {
        None
    } else {
        Some(
            total_bytes
                .saturating_sub(bytes_transferred)
                .checked_div(bytes_per_second)
                .unwrap_or(0),
        )
    };

    TransferProgress {
        bytes_transferred,
        total_bytes,
        bytes_per_second,
        eta_seconds,
        sha256,
    }
}

fn progress_speed(bytes_transferred: u64, started_at: std::time::Instant) -> u64 {
    let elapsed = started_at.elapsed().as_secs().max(1);
    bytes_transferred / elapsed
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TransferHeader {
    filename: String,
    total_bytes: u64,
    resume_from: u64,
    sha256: String,
}

async fn write_header(stream: &mut TcpStream, header: &TransferHeader) -> anyhow::Result<()> {
    let payload = serde_json::to_vec(header)?;
    let len = payload.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(&payload).await?;
    Ok(())
}

async fn read_header(stream: &mut TcpStream) -> anyhow::Result<TransferHeader> {
    let mut len_buf = [0_u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut payload = vec![0_u8; len];
    stream.read_exact(&mut payload).await?;
    serde_json::from_slice(&payload).context("invalid transfer header")
}

async fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let mut file = fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0_u8; 64 * 1024];

    loop {
        let read = file.read(&mut buf).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn safe_output_path(output_dir: &Path, filename: &str) -> PathBuf {
    let filename = Path::new(filename)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("flowhub-transfer.bin");
    output_dir.join(filename)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn sends_and_receives_file() {
        let source_dir = tempdir().unwrap();
        let target_dir = tempdir().unwrap();
        let source_path = source_dir.path().join("hello.txt");
        fs::write(&source_path, b"flowhub").await.unwrap();

        let receive_dir = target_dir.path().to_path_buf();
        let receiver = tokio::spawn(async move {
            receive_file("127.0.0.1:47611", receive_dir, TransferControl::default())
                .await
                .unwrap()
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let sent = send_file(
            "127.0.0.1:47611",
            &source_path,
            0,
            TransferControl::default(),
        )
        .await
        .unwrap();
        let received = receiver.await.unwrap();

        assert_eq!(sent.sha256, received.sha256);
        assert_eq!(
            fs::read(target_dir.path().join("hello.txt")).await.unwrap(),
            b"flowhub"
        );
    }
}
