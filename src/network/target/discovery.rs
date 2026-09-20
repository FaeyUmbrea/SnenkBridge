use std::{
    collections::BTreeMap,
    net::{IpAddr, UdpSocket},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde_json::Value;

use crate::network::{cancelled, set_status, transient, NetworkStatus, Status, POLL};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredTarget {
    pub host: String,
    pub port: u16,
    pub active: bool,
    pub instance_id: String,
    pub window_title: String,
}

pub fn decode_discovery(bytes: &[u8], host: IpAddr) -> Option<DiscoveredTarget> {
    let v: Value = serde_json::from_slice(bytes).ok()?;
    if v["apiName"] != "VTubeStudioPublicAPI" || v["messageType"] != "VTubeStudioAPIStateBroadcast"
    {
        return None;
    }
    let d = &v["data"];
    Some(DiscoveredTarget {
        host: host.to_string(),
        port: u16::try_from(d["port"].as_u64()?).ok().filter(|p| *p > 0)?,
        active: d["active"].as_bool()?,
        instance_id: d["instanceID"].as_str()?.into(),
        window_title: d["windowTitle"].as_str()?.into(),
    })
}

/// Discovery can change the port, never the host chosen by the user.
pub fn discover_port(host: IpAddr, stop: &AtomicBool) -> Option<u16> {
    let socket = UdpSocket::bind(if host.is_ipv4() {
        "0.0.0.0:47779"
    } else {
        "[::]:47779"
    })
    .ok()?;
    socket.set_read_timeout(Some(POLL)).ok()?;
    let deadline = Instant::now() + Duration::from_millis(2200);
    let mut bytes = [0; 8192];
    while !cancelled(stop) && Instant::now() < deadline {
        match socket.recv_from(&mut bytes) {
            Ok((n, from)) if from.ip() == host => {
                if let Some(target) = decode_discovery(&bytes[..n], from.ip()) {
                    if target.active {
                        return Some(target.port);
                    }
                }
            }
            Ok(_) => {}
            Err(e) if transient(&e) => {}
            Err(_) => return None,
        }
    }
    None
}

#[allow(dead_code)]
pub struct DiscoveryHandle {
    stop: Arc<AtomicBool>,
    status: Status,
    targets: Arc<Mutex<BTreeMap<String, (DiscoveredTarget, Instant)>>>,
    worker: Option<JoinHandle<()>>,
}

#[allow(dead_code)]
impl DiscoveryHandle {
    pub fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(NetworkStatus::Connecting));
        let targets = Arc::new(Mutex::new(BTreeMap::new()));
        let (s, st, out) = (stop.clone(), status.clone(), targets.clone());
        let worker = thread::spawn(move || {
            let result = (|| -> Result<(), String> {
                let socket = UdpSocket::bind("0.0.0.0:47779").map_err(|e| e.to_string())?;
                socket
                    .set_read_timeout(Some(POLL))
                    .map_err(|e| e.to_string())?;
                let mut bytes = [0u8; 8192];
                set_status(&st, NetworkStatus::Connected);
                while !cancelled(&s) {
                    match socket.recv_from(&mut bytes) {
                        Ok((n, from)) => {
                            if let Ok(v) = serde_json::from_slice::<Value>(&bytes[..n]) {
                                if v["apiName"] == "VTubeStudioPublicAPI"
                                    && v["messageType"] == "VTubeStudioAPIStateBroadcast"
                                {
                                    let d = &v["data"];
                                    if let (Some(port), Some(active), Some(id), Some(title)) = (
                                        d["port"].as_u64().filter(|p| *p > 0 && *p <= 65535),
                                        d["active"].as_bool(),
                                        d["instanceID"].as_str(),
                                        d["windowTitle"].as_str(),
                                    ) {
                                        out.lock().unwrap().insert(
                                            id.to_owned(),
                                            (
                                                DiscoveredTarget {
                                                    host: from.ip().to_string(),
                                                    port: port as u16,
                                                    active,
                                                    instance_id: id.into(),
                                                    window_title: title.into(),
                                                },
                                                Instant::now(),
                                            ),
                                        );
                                    }
                                }
                            }
                        }
                        Err(e) if transient(&e) => {}
                        Err(e) => return Err(e.to_string()),
                    }
                    out.lock()
                        .unwrap()
                        .retain(|_, (_, t)| t.elapsed() < Duration::from_secs(6));
                }
                Ok(())
            })();
            set_status(
                &st,
                match result {
                    Ok(()) => NetworkStatus::Stopped,
                    Err(e) => NetworkStatus::Error(e),
                },
            );
        });
        Self {
            stop,
            status,
            targets,
            worker: Some(worker),
        }
    }

    pub fn snapshot(&self) -> (NetworkStatus, Vec<DiscoveredTarget>) {
        (
            self.status.lock().unwrap().clone(),
            self.targets
                .lock()
                .unwrap()
                .values()
                .map(|(t, _)| t.clone())
                .collect(),
        )
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.worker.take() {
            let _ = t.join();
        }
        set_status(&self.status, NetworkStatus::Stopped);
    }
}

impl Drop for DiscoveryHandle {
    fn drop(&mut self) {
        self.stop();
    }
}
