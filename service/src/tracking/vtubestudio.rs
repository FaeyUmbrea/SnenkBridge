use std::{
    net::UdpSocket,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
        Arc,
    },
    time,
};

use log::warn;

use crate::tracking::{client::TrackingClient, response::TrackingResponse};

pub struct VTubeStudioTrackingClient;

impl TrackingClient for VTubeStudioTrackingClient {
    fn run(ip: String, sender: Sender<TrackingResponse>, active: Arc<AtomicBool>) {
        let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let _ = socket.set_read_timeout(Some(time::Duration::new(2, 0)));
        let port = socket.local_addr().unwrap().port();

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
                    Ok(_) => {
                        // nice
                    }
                    Err(error) => {
                        warn!("Unable to request tracking data: {}", error) // Maybe reconnect
                    }
                }
            }

            match socket.recv_from(&mut buf) {
                Ok((amt, _src)) => match serde_json::from_slice::<TrackingResponse>(&buf[..amt]) {
                    Ok(data) => Self::send(&sender, data),
                    Err(error) => {
                        warn!("Unnable to deserialize: {}", error)
                    }
                },
                Err(error) => {
                    warn!("Unnable to receive: {}", error) // Maybe reconnect
                }
            }
        }
    }
}
