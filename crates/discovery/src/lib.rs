use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::time;
use uuid::Uuid;

pub const FLOWHUB_VERSION: &str = "0.1.0";
pub const DEFAULT_DISCOVERY_PORT: u16 = 47321;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerInfo {
    pub device_id: String,
    pub hostname: String,
    pub ip: IpAddr,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryMessage {
    pub device_id: String,
    pub hostname: String,
    pub version: String,
}

#[derive(Debug, Clone)]
struct PeerEntry {
    peer: PeerInfo,
    last_seen: Instant,
}

#[derive(Clone)]
pub struct DiscoveryService {
    local: DiscoveryMessage,
    peers: Arc<Mutex<HashMap<String, PeerEntry>>>,
    port: u16,
    ttl: Duration,
}

impl DiscoveryService {
    pub fn new(port: u16) -> anyhow::Result<Self> {
        let hostname = hostname::get()
            .context("failed to read hostname")?
            .to_string_lossy()
            .to_string();

        Ok(Self {
            local: DiscoveryMessage {
                device_id: Uuid::new_v4().to_string(),
                hostname,
                version: FLOWHUB_VERSION.to_string(),
            },
            peers: Arc::new(Mutex::new(HashMap::new())),
            port,
            ttl: Duration::from_secs(20),
        })
    }

    pub fn with_local(local: DiscoveryMessage, port: u16) -> Self {
        Self {
            local,
            peers: Arc::new(Mutex::new(HashMap::new())),
            port,
            ttl: Duration::from_secs(20),
        }
    }

    pub fn local_device_id(&self) -> &str {
        &self.local.device_id
    }

    pub fn list_peers(&self) -> Vec<PeerInfo> {
        let mut peers = self.peers.lock().expect("peer lock poisoned");
        let now = Instant::now();
        peers.retain(|_, entry| now.duration_since(entry.last_seen) <= self.ttl);

        let mut values = peers
            .values()
            .map(|entry| entry.peer.clone())
            .collect::<Vec<_>>();
        values.sort_by(|a, b| a.hostname.cmp(&b.hostname));
        values
    }

    pub fn observe_message(&self, message: DiscoveryMessage, source: SocketAddr) {
        if message.device_id == self.local.device_id {
            return;
        }

        let peer = PeerInfo {
            device_id: message.device_id.clone(),
            hostname: message.hostname,
            ip: source.ip(),
            version: message.version,
        };

        self.peers.lock().expect("peer lock poisoned").insert(
            message.device_id,
            PeerEntry {
                peer,
                last_seen: Instant::now(),
            },
        );
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, self.port)).await?;
        socket.set_broadcast(true)?;

        let broadcast_target = SocketAddr::from((Ipv4Addr::BROADCAST, self.port));
        let mut interval = time::interval(Duration::from_secs(5));
        let mut buf = vec![0_u8; 2048];

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let payload = serde_json::to_vec(&self.local)?;
                    socket.send_to(&payload, broadcast_target).await?;
                }
                result = socket.recv_from(&mut buf) => {
                    let (len, source) = result?;
                    if let Ok(message) = serde_json::from_slice::<DiscoveryMessage>(&buf[..len]) {
                        self.observe_message(message, source);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observes_remote_peer() {
        let service = DiscoveryService::with_local(
            DiscoveryMessage {
                device_id: "local".into(),
                hostname: "me".into(),
                version: "0.1.0".into(),
            },
            0,
        );

        service.observe_message(
            DiscoveryMessage {
                device_id: "remote".into(),
                hostname: "desk".into(),
                version: "0.1.0".into(),
            },
            "192.168.1.10:47321".parse().unwrap(),
        );

        let peers = service.list_peers();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].hostname, "desk");
    }

    #[test]
    fn ignores_own_broadcast() {
        let service = DiscoveryService::with_local(
            DiscoveryMessage {
                device_id: "local".into(),
                hostname: "me".into(),
                version: "0.1.0".into(),
            },
            0,
        );

        service.observe_message(
            DiscoveryMessage {
                device_id: "local".into(),
                hostname: "me".into(),
                version: "0.1.0".into(),
            },
            "127.0.0.1:47321".parse().unwrap(),
        );

        assert!(service.list_peers().is_empty());
    }
}
