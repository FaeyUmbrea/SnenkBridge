mod ifacial;
mod vts;

#[cfg(test)]
mod tests;

pub use ifacial::parse_ifacial;
pub use vts::parse_vts_tracking;

use std::{
    io::Read,
    net::{TcpListener, UdpSocket},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde_json::json;

use super::{
    cancelled, pause, resolve, transient, NetworkStatus, SourceKind, SourceSettings, MAX_FRAME,
    POLL,
};
use crate::model::TrackingFrame;
use ifacial::Frames;

struct SourceState {
    status: NetworkStatus,
    latest: Option<TrackingFrame>,
    count: u64,
}

pub struct SourceHandle {
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<SourceState>>,
    worker: Option<JoinHandle<()>>,
}

impl SourceHandle {
    pub fn start(settings: SourceSettings) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let state = Arc::new(Mutex::new(SourceState {
            status: NetworkStatus::Connecting,
            latest: None,
            count: 0,
        }));
        let s = stop.clone();
        let data = state.clone();
        let worker = thread::spawn(move || {
            while !cancelled(&s) {
                let result = run(&settings, &s, &data);
                if let Err(e) = result {
                    data.lock().unwrap().status = NetworkStatus::Error(e);
                }
                pause(&s, Duration::from_secs(1));
            }
            data.lock().unwrap().status = NetworkStatus::Stopped;
        });
        Self {
            stop,
            state,
            worker: Some(worker),
        }
    }

    pub fn snapshot(&self) -> (NetworkStatus, Option<TrackingFrame>, u64) {
        let s = self.state.lock().unwrap();
        (s.status.clone(), s.latest.clone(), s.count)
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.worker.take() {
            let _ = t.join();
        }
    }
}

impl Drop for SourceHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

fn receive(state: &Mutex<SourceState>, frame: Result<TrackingFrame, String>) {
    if let Ok(frame) = frame {
        let mut s = state.lock().unwrap();
        s.latest = Some(frame);
        s.count = s.count.saturating_add(1);
        s.status = NetworkStatus::Connected;
    }
}

fn run(
    settings: &SourceSettings,
    stop: &AtomicBool,
    state: &Mutex<SourceState>,
) -> Result<(), String> {
    let port = if settings.kind == SourceKind::VTubeStudio {
        21412
    } else {
        49983
    };
    let peer = resolve(&settings.phone_address, port, stop)?;
    let bind = if peer.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let udp = UdpSocket::bind(bind).map_err(|e| e.to_string())?;
    udp.set_read_timeout(Some(POLL))
        .map_err(|e| e.to_string())?;
    udp.set_write_timeout(Some(POLL))
        .map_err(|e| e.to_string())?;
    let mut bytes = [0u8; MAX_FRAME];
    if settings.kind == SourceKind::VTubeStudio {
        let request = serde_json::to_vec(&json!({
            "messageType": "iOSTrackingDataRequest",
            "time": 5.0,
            "sentBy": "SnenkBridge",
            "ports": [udp.local_addr().map_err(|e| e.to_string())?.port()]
        }))
        .unwrap();
        let mut requested = Instant::now() - Duration::from_secs(2);
        let mut last = Instant::now();
        while !cancelled(stop) {
            if requested.elapsed() >= Duration::from_secs(1) {
                udp.send_to(&request, peer).map_err(|e| e.to_string())?;
                requested = Instant::now();
            }
            match udp.recv_from(&mut bytes) {
                Ok((n, from)) if from.ip() == peer.ip() => {
                    let frame = parse_vts_tracking(&bytes[..n]);
                    if frame.is_ok() {
                        last = Instant::now();
                    }
                    receive(state, frame);
                }
                Ok(_) => {}
                Err(e) if transient(&e) => {}
                Err(e) => return Err(e.to_string()),
            }
            if last.elapsed() > Duration::from_secs(3) {
                state.lock().unwrap().status =
                    NetworkStatus::Error("Tracking interrupted: awaiting phone data".into());
            }
        }
    } else {
        let listener = TcpListener::bind(if peer.is_ipv4() {
            "0.0.0.0:49986"
        } else {
            "[::]:49986"
        })
        .map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let start =
            b"iFacialMocap_UDPTCP_sahuasouryya9218sauhuiayeta91555dy3719|sendDataVersion=v2";
        let result = (|| {
            let mut request = Instant::now() - Duration::from_secs(3);
            while !cancelled(stop) {
                if request.elapsed() > Duration::from_secs(2) {
                    udp.send_to(start, peer).map_err(|e| e.to_string())?;
                    request = Instant::now();
                }
                match listener.accept() {
                    Ok((mut stream, from)) if from.ip() == peer.ip() => {
                        stream
                            .set_read_timeout(Some(POLL))
                            .map_err(|e| e.to_string())?;
                        let mut frames = Frames::default();
                        let mut last = Instant::now();
                        while !cancelled(stop) {
                            match stream.read(&mut bytes) {
                                Ok(0) => break,
                                Ok(n) => {
                                    for f in frames.push(&bytes[..n]) {
                                        let parsed = parse_ifacial(&f);
                                        if parsed.is_ok() {
                                            last = Instant::now();
                                        }
                                        receive(state, parsed);
                                    }
                                }
                                Err(e) if transient(&e) => {}
                                Err(e) => return Err(e.to_string()),
                            }
                            if last.elapsed() > Duration::from_secs(3) {
                                state.lock().unwrap().status = NetworkStatus::Error(
                                    "Tracking interrupted: awaiting phone data".into(),
                                );
                                break;
                            }
                        }
                        state.lock().unwrap().status = NetworkStatus::Connecting;
                    }
                    Ok(_) => {}
                    Err(e) if transient(&e) => pause(stop, POLL),
                    Err(e) => return Err(e.to_string()),
                }
            }
            Ok(())
        })();
        let _ = udp.send_to(
            b"iFacialMocap_UDPTCPSTOP_sahuasouryya9218sauhuiayeta91555dy3719",
            peer,
        );
        result?;
    }
    Ok(())
}
