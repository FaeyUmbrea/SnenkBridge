use std::{
    net::UdpSocket,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
        Arc,
    },
    time,
};

use log::{info, warn};

use crate::tracking::{client::TrackingClient, response::TrackingResponse};

pub struct VTubeStudioTrackingClient;

impl TrackingClient for VTubeStudioTrackingClient {
    fn run(ip: String, sender: Sender<TrackingResponse>, active: Arc<AtomicBool>) {
        while active.load(Ordering::Relaxed) {
            let socket = match UdpSocket::bind("0.0.0.0:0") {
                Ok(s) => s,
                Err(e) => {
                    warn!("Failed to bind UDP socket: {}, retrying...", e);
                    std::thread::sleep(time::Duration::from_secs(3));
                    continue;
                }
            };
            let _ = socket.set_read_timeout(Some(time::Duration::new(2, 0)));
            let port = match socket.local_addr() {
                Ok(addr) => addr.port(),
                Err(e) => {
                    warn!("Failed to get local address: {}, retrying...", e);
                    std::thread::sleep(time::Duration::from_secs(3));
                    continue;
                }
            };

            info!("VTS tracking client bound on port {}, target {}", port, ip);

            let mut buf = [0; 4096];

            let request_traking: String = serde_json::json!({
                "messageType":"iOSTrackingDataRequest",
                "sentBy": "SnenkBridge",
                "sendForSeconds": 10,
                "ports": [port]
            })
            .to_string();

            let mut next_time = time::Instant::now();

            while active.load(Ordering::Relaxed) {
                if next_time <= time::Instant::now() {
                    next_time = time::Instant::now() + time::Duration::from_secs(1);

                    match socket.send_to(request_traking.as_bytes(), format!("{:}:21412", ip)) {
                        Ok(_) => {}
                        Err(error) => {
                            warn!("Unable to request tracking data: {}", error)
                        }
                    }
                }

                match socket.recv_from(&mut buf) {
                    Ok((amt, _src)) => {
                        match serde_json::from_slice::<TrackingResponse>(&buf[..amt]) {
                            Ok(data) => Self::send(&sender, data),
                            Err(error) => {
                                warn!("Unable to deserialize: {}", error)
                            }
                        }
                    }
                    Err(_) => {} // timeout, just loop
                }
            }
            break;
        }
    }
}
