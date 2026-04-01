use crate::{
    tracking::{
        client::TrackingClient,
        response::{Cords, Shape, TrackingResponse},
    },
    utils::get_current_timestamp,
};
use regex::Regex;
use std::{collections::HashMap, sync::atomic::Ordering};

use std::{
    io::Read,
    net::{TcpListener, UdpSocket},
    sync::{atomic::AtomicBool, mpsc::Sender, Arc},
    thread, time,
};

use log::{info, warn};

#[derive(Clone)]
pub struct IFacialMocapTrackingClinet;

impl TrackingClient for IFacialMocapTrackingClinet {
    fn run(ip: String, sender: Sender<TrackingResponse>, active: Arc<AtomicBool>) {
        while active.load(Ordering::Relaxed) {
            // UDP discovery
            let ip_clone = ip.clone();
            let udp_result = thread::spawn(move || -> Result<(), String> {
                let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
                let _ = socket.set_read_timeout(Some(time::Duration::new(2, 0)));
                let message = "iFacialMocap_UDPTCP_sahuasouryya9218sauhuiayeta91555dy3719";
                let destination_address = format!("{}:{}", ip_clone, 49983);
                socket
                    .send_to(message.as_bytes(), &destination_address)
                    .map_err(|e| format!("Failed to send UDP message: {}", e))?;
                info!("iFacialMocap UDP discovery sent to {}", destination_address);
                Ok(())
            })
            .join();

            match udp_result {
                Ok(Err(e)) => {
                    warn!("iFacialMocap UDP discovery failed: {}, retrying...", e);
                    thread::sleep(time::Duration::from_secs(3));
                    continue;
                }
                Err(_) => {
                    warn!("iFacialMocap UDP thread panicked, retrying...");
                    thread::sleep(time::Duration::from_secs(3));
                    continue;
                }
                Ok(Ok(())) => {}
            }

            // TCP Server
            let address = format!("0.0.0.0:{}", 49986);
            let listener = match TcpListener::bind(&address) {
                Ok(l) => l,
                Err(e) => {
                    warn!("Failed to bind TCP listener on {}: {}, retrying...", address, e);
                    thread::sleep(time::Duration::from_secs(3));
                    continue;
                }
            };
            let _ = listener.set_nonblocking(true);
            info!("iFacialMocap TCP server listening on {}", address);

            let re = Regex::new("___iFacialMocaptrackingStatus-[01]\\|").unwrap();

            while active.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _addr)) => {
                        let sender_clone = sender.clone();
                        let re = re.clone();
                        let active_clone = Arc::clone(&active);
                        thread::spawn(move || {
                            let mut partial_buffer = String::new();
                            let mut buffer = [0; 8192];

                            while active_clone.load(Ordering::Relaxed) {
                                match &stream.read(&mut buffer) {
                                    Ok(0) => break,
                                    Ok(n) => {
                                        if let Ok(raw_data) =
                                            String::from_utf8(buffer[..*n].to_vec())
                                        {
                                            partial_buffer.push_str(&raw_data);

                                            let mut matches: Vec<_> =
                                                re.find_iter(&partial_buffer).collect();
                                            while matches.len() >= 2 {
                                                let first_start = matches[0].start();
                                                let second_start = matches[1].start();

                                                let data_to_parse =
                                                    &partial_buffer[first_start..second_start];
                                                if let Ok(d) =
                                                    parse_tracking_string(data_to_parse)
                                                {
                                                    Self::send(&sender_clone, d);

                                                    partial_buffer
                                                        .replace_range(0..second_start, "");
                                                    matches =
                                                        re.find_iter(&partial_buffer).collect();
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to read from socket: {:?}", e);
                                        break;
                                    }
                                }
                            }
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(time::Duration::from_millis(100));
                    }
                    Err(e) => {
                        warn!("Failed to accept connection: {:?}", e);
                    }
                }
            }
            break;
        }
    }
}

fn get_coordinate_values(part: &str) -> Result<(String, Vec<f64>), Box<dyn std::error::Error>> {
    let mut split_iter = part.split('#');
    let name = split_iter.next().ok_or("Missing name")?.to_string();
    let values_str = split_iter.next().ok_or("Missing values")?;
    let values: Vec<f64> = values_str
        .split(',')
        .map(|v| v.parse::<f64>())
        .collect::<Result<Vec<f64>, _>>()?;
    Ok((name, values))
}

fn capitalize_first_letter(string: &str) -> String {
    let mut characters = string.chars();
    match characters.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
    }
}

fn parse_tracking_string(string: &str) -> Result<TrackingResponse, Box<dyn std::error::Error>> {
    let parts: Vec<&str> = string.split('=').collect();
    if parts.len() != 2 {
        return Err("Invalid input string format".into());
    }

    let status_part = parts[0];
    let coords_part = parts[1];

    let mut status_map: HashMap<String, i16> = HashMap::new();
    for item in status_part.trim().split('|') {
        if !item.is_empty() {
            let kv: Vec<&str> = item.split('-').collect();
            if kv.len() == 2 {
                let mut key = capitalize_first_letter(kv[0]);
                if !key.eq("___iFacialMocaptrackingStatus-0") && key.contains("_") {
                    let end_index = key.len();
                    let start_index = end_index - 2;

                    if key.ends_with("_L") {
                        key.replace_range(start_index..end_index, "Left");
                    } else if key.ends_with("_R") {
                        key.replace_range(start_index..end_index, "Right");
                    }
                }

                let value: i16 = kv[1].parse()?;
                status_map.insert(key, value);
            }
        }
    }

    let coords_parts: Vec<&str> = coords_part.trim().split('|').collect();

    let (_, head_values) = get_coordinate_values(
        coords_parts
            .first()
            .ok_or("Missing coordinates for \"head\"")?,
    )?;
    let head_rotation = Cords {
        // I really don't understand why that thing reverted
        x: head_values[1],
        y: head_values[0],
        z: head_values[2],
    };
    let head_position = Cords {
        x: head_values[3],
        y: head_values[4],
        z: head_values[5],
    };

    // Useless thing for now
    let (_, right_eye_values) = get_coordinate_values(
        coords_parts
            .get(1)
            .ok_or("Missing coordinates for \"rightEye\"")?,
    )?;
    let (_, left_eye_values) = get_coordinate_values(
        coords_parts
            .get(2)
            .ok_or("Missing coordinates for \"leftEye\"")?,
    )?;
    let left_eye = Cords {
        x: left_eye_values[0],
        y: left_eye_values[1],
        z: left_eye_values[2],
    };

    let face_found = status_map
        .get("___iFacialMocaptrackingStatus")
        .copied()
        .unwrap_or(0)
        == 1;

    let mut blend_shapes: Vec<Shape> = status_map
        .iter()
        .filter(|(k, _)| !k.starts_with("___"))
        .map(|(k, v)| Shape {
            k: k.clone(),
            v: (*v as f64) / 100.0, // magic value, nevermind
        })
        .collect();
    blend_shapes.push(Shape {
        k: "RightEyeX".into(),
        v: right_eye_values[0],
    });
    blend_shapes.push(Shape {
        k: "RightEyeY".into(),
        v: right_eye_values[1],
    });
    blend_shapes.push(Shape {
        k: "RightEyeZ".into(),
        v: right_eye_values[2],
    });
    blend_shapes.push(Shape {
        k: "LeftEyeX".into(),
        v: left_eye_values[0],
    });
    blend_shapes.push(Shape {
        k: "LeftEyeY".into(),
        v: left_eye_values[1],
    });
    blend_shapes.push(Shape {
        k: "LeftEyeZ".into(),
        v: left_eye_values[0],
    });

    let timestamp = get_current_timestamp();

    Ok(TrackingResponse {
        timestamp,
        hotkey: 0,
        face_found,
        rotation: head_rotation,
        position: head_position,
        eye_left: left_eye,
        blend_shapes,
    })
}
