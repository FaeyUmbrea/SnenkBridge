//! Phone tracking and VTube Studio transports. All sockets live on worker threads.
mod source;
mod target;
#[allow(unused_imports)]
pub use source::{parse_ifacial, parse_vts_tracking, SourceHandle};
use std::{
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
#[allow(unused_imports)]
pub use target::{DiscoveredTarget, DiscoveryHandle, TargetHandle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    VTubeStudio,
    IFacialMocap,
}
#[derive(Clone, Debug)]
pub struct SourceSettings {
    pub kind: SourceKind,
    pub phone_address: String,
}
#[derive(Clone, Debug)]
pub struct TargetSettings {
    pub host: String,
    pub port: u16,
    pub token_path: PathBuf,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetworkStatus {
    Stopped,
    Connecting,
    Authorizing,
    Connected,
    Error(String),
}
pub(crate) const POLL: Duration = Duration::from_millis(50);
pub(crate) const MAX_FRAME: usize = 65536;
pub(crate) type Status = Arc<Mutex<NetworkStatus>>;
pub(crate) fn set_status(status: &Status, value: NetworkStatus) {
    *status.lock().unwrap_or_else(|e| e.into_inner()) = value;
}
pub(crate) fn cancelled(stop: &AtomicBool) -> bool {
    stop.load(Ordering::Acquire)
}
pub(crate) fn pause(stop: &AtomicBool, duration: Duration) {
    let until = Instant::now() + duration;
    while !cancelled(stop) && Instant::now() < until {
        thread::sleep(POLL.min(until.saturating_duration_since(Instant::now())));
    }
}
pub(crate) fn transient(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::Interrupted
    )
}
pub(crate) fn resolve(host: &str, port: u16, stop: &AtomicBool) -> Result<SocketAddr, String> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, port));
    }
    if host == "localhost" {
        return Ok(SocketAddr::from(([127, 0, 0, 1], port)));
    }
    // OS DNS has no cancellation API; isolate it so stop never waits on DNS.
    let host = host.to_owned();
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    thread::spawn(move || {
        let result = (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|e| e.to_string())
            .and_then(|mut a| a.next().ok_or("No addresses found".into()));
        let _ = tx.send(result);
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    while !cancelled(stop) && Instant::now() < deadline {
        match rx.recv_timeout(POLL) {
            Ok(v) => return v,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err("Address lookup failed".into())
            }
            Err(_) => {}
        }
    }
    Err("Address lookup cancelled or timed out".into())
}
