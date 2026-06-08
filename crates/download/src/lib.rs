use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DownloadProtocol {
    Http,
    Https,
    Ftp,
    Magnet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadStatus {
    pub gid: String,
    pub status: String,
    pub total_length: u64,
    pub completed_length: u64,
    pub download_speed: u64,
    pub upload_speed: u64,
    pub connections: u64,
    pub files: Vec<String>,
}

#[derive(Clone)]
pub struct Aria2Client {
    endpoint: String,
    secret: Option<String>,
}

impl Aria2Client {
    pub fn new(endpoint: impl Into<String>, secret: Option<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            secret,
        }
    }

    pub async fn add_url(&self, url: &str) -> anyhow::Result<String> {
        validate_download_url(url)?;
        let params = self.with_secret(json!([[url]]));
        let value = self.call("aria2.addUri", params).await?;
        value
            .as_str()
            .map(ToOwned::to_owned)
            .ok_or_else(|| anyhow!("aria2.addUri returned non-string gid"))
    }

    pub async fn pause(&self, gid: &str) -> anyhow::Result<()> {
        self.call("aria2.pause", self.with_secret(json!([gid])))
            .await?;
        Ok(())
    }

    pub async fn resume(&self, gid: &str) -> anyhow::Result<()> {
        self.call("aria2.unpause", self.with_secret(json!([gid])))
            .await?;
        Ok(())
    }

    /// Returns true if the aria2 RPC server is reachable.
    pub async fn ping(&self) -> bool {
        self.call("aria2.getVersion", json!([])).await.is_ok()
    }

    pub async fn remove(&self, gid: &str) -> anyhow::Result<()> {
        self.call("aria2.remove", self.with_secret(json!([gid])))
            .await?;
        Ok(())
    }

    pub async fn status(&self, gid: &str) -> anyhow::Result<DownloadStatus> {
        let keys = [
            "gid",
            "status",
            "totalLength",
            "completedLength",
            "downloadSpeed",
            "uploadSpeed",
            "connections",
            "files",
        ];
        let value = self
            .call("aria2.tellStatus", self.with_secret(json!([gid, keys])))
            .await?;
        parse_status(value)
    }

    async fn call(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let request = json!({
            "jsonrpc": "2.0",
            "id": NEXT_ID.fetch_add(1, Ordering::Relaxed),
            "method": method,
            "params": params,
        });

        post_json(&self.endpoint, &request).await
    }

    fn with_secret(&self, params: Value) -> Value {
        match (&self.secret, params) {
            (Some(secret), Value::Array(mut values)) => {
                values.insert(0, Value::String(format!("token:{secret}")));
                Value::Array(values)
            }
            (_, other) => other,
        }
    }
}

pub fn validate_download_url(url: &str) -> anyhow::Result<DownloadProtocol> {
    if url.starts_with("http://") {
        Ok(DownloadProtocol::Http)
    } else if url.starts_with("https://") {
        Ok(DownloadProtocol::Https)
    } else if url.starts_with("ftp://") {
        Ok(DownloadProtocol::Ftp)
    } else if url.starts_with("magnet:?") {
        Ok(DownloadProtocol::Magnet)
    } else {
        Err(anyhow!("unsupported download URL protocol"))
    }
}

fn parse_status(value: Value) -> anyhow::Result<DownloadStatus> {
    let file_paths = value
        .get("files")
        .and_then(Value::as_array)
        .map(|files| {
            files
                .iter()
                .filter_map(|file| {
                    file.get("path")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(DownloadStatus {
        gid: string_field(&value, "gid")?,
        status: string_field(&value, "status")?,
        total_length: number_string_field(&value, "totalLength")?,
        completed_length: number_string_field(&value, "completedLength")?,
        download_speed: number_string_field(&value, "downloadSpeed")?,
        upload_speed: number_string_field(&value, "uploadSpeed")?,
        connections: number_string_field(&value, "connections")?,
        files: file_paths,
    })
}

fn string_field(value: &Value, key: &str) -> anyhow::Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .with_context(|| format!("missing aria2 field {key}"))
}

fn number_string_field(value: &Value, key: &str) -> anyhow::Result<u64> {
    let raw = value.get(key).and_then(Value::as_str).unwrap_or("0");
    raw.parse::<u64>()
        .with_context(|| format!("invalid aria2 numeric field {key}"))
}

async fn post_json(endpoint: &str, body: &Value) -> anyhow::Result<Value> {
    let endpoint = endpoint.strip_prefix("http://").unwrap_or(endpoint);
    let (host_port, path) = endpoint.split_once('/').unwrap_or((endpoint, "jsonrpc"));
    let mut stream = TcpStream::connect(host_port)
        .await
        .with_context(|| format!("failed to connect to aria2 RPC at {host_port}"))?;
    let body = serde_json::to_vec(body)?;

    let request = format!(
        "POST /{path} HTTP/1.1\r\nHost: {host_port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(request.as_bytes()).await?;
    stream.write_all(&body).await?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    let response = String::from_utf8(response)?;
    let (_, payload) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| anyhow!("invalid aria2 HTTP response"))?;
    let rpc: Value = serde_json::from_str(payload)?;

    if let Some(error) = rpc.get("error") {
        return Err(anyhow!("aria2 RPC error: {error}"));
    }

    rpc.get("result")
        .cloned()
        .ok_or_else(|| anyhow!("aria2 RPC response missing result"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_supported_protocols() {
        assert_eq!(
            validate_download_url("http://example.com/a").unwrap(),
            DownloadProtocol::Http
        );
        assert_eq!(
            validate_download_url("https://example.com/a").unwrap(),
            DownloadProtocol::Https
        );
        assert_eq!(
            validate_download_url("ftp://example.com/a").unwrap(),
            DownloadProtocol::Ftp
        );
        assert_eq!(
            validate_download_url("magnet:?xt=urn:btih:abc").unwrap(),
            DownloadProtocol::Magnet
        );
        assert!(validate_download_url("file:///tmp/a").is_err());
    }

    #[test]
    fn parses_status_payload() {
        let status = parse_status(json!({
            "gid": "1",
            "status": "active",
            "totalLength": "100",
            "completedLength": "25",
            "downloadSpeed": "5",
            "uploadSpeed": "0",
            "connections": "1",
            "files": [{"path": "/tmp/a.iso"}]
        }))
        .unwrap();

        assert_eq!(status.completed_length, 25);
        assert_eq!(status.files, vec!["/tmp/a.iso"]);
    }
}
