use super::*;
use crate::model::TrackingFrame;
use serde_json::{json, Value};
use std::{
    io::Read,
    net::{TcpListener, UdpSocket},
    thread::JoinHandle,
};

fn number(v: &Value) -> Result<f64, String> {
    v.as_f64()
        .filter(|n| n.is_finite())
        .ok_or("Invalid numeric tracking value".into())
}
fn canonical(name: &str) -> Option<String> {
    let name = if let Some(n) = name.strip_suffix("_L") {
        format!("{n}Left")
    } else if let Some(n) = name.strip_suffix("_R") {
        format!("{n}Right")
    } else {
        name.to_owned()
    };
    let mut chars = name.chars();
    let name = format!("{}{}", chars.next()?.to_ascii_uppercase(), chars.as_str());
    let paired = [
        "EyeBlink",
        "EyeLookDown",
        "EyeLookIn",
        "EyeLookOut",
        "EyeLookUp",
        "EyeSquint",
        "EyeWide",
        "BrowDown",
        "BrowOuterUp",
        "CheekSquint",
        "MouthDimple",
        "MouthFrown",
        "MouthLowerDown",
        "MouthPress",
        "MouthSmile",
        "MouthStretch",
        "MouthUpperUp",
        "NoseSneer",
    ];
    let single = [
        "BrowInnerUp",
        "CheekPuff",
        "JawForward",
        "JawLeft",
        "JawOpen",
        "JawRight",
        "MouthClose",
        "MouthFunnel",
        "MouthLeft",
        "MouthPucker",
        "MouthRight",
        "MouthRollLower",
        "MouthRollUpper",
        "MouthShrugLower",
        "MouthShrugUpper",
        "TongueOut",
    ];
    if single.contains(&name.as_str())
        || paired
            .iter()
            .any(|p| name == format!("{p}Left") || name == format!("{p}Right"))
    {
        Some(name)
    } else {
        None
    }
}
fn vector(frame: &mut TrackingFrame, prefix: &str, values: &[f64]) {
    for (axis, n) in ["X", "Y", "Z"].iter().zip(values) {
        frame.values.insert(format!("{prefix}{axis}"), *n);
    }
}
/// Preserve the vendor's raw axes; no undocumented sign or unit conversion.
pub fn parse_vts_tracking(bytes: &[u8]) -> Result<TrackingFrame, String> {
    if bytes.len() > MAX_FRAME {
        return Err("Tracking packet too large".into());
    }
    let data: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let face = data["FaceFound"].as_bool().ok_or("Missing FaceFound")?;
    let mut frame = TrackingFrame {
        face_present: face,
        ..Default::default()
    };
    frame
        .values
        .insert("FaceFound".into(), if face { 1.0 } else { 0.0 });
    for (key, prefix) in [
        ("Position", "HeadPos"),
        ("Rotation", "HeadRot"),
        ("EyeLeft", "EyeLeftRot"),
        ("EyeRight", "EyeRightRot"),
    ] {
        if data.get(key).is_some() {
            let values = [
                number(&data[key]["x"])?,
                number(&data[key]["y"])?,
                number(&data[key]["z"])?,
            ];
            vector(&mut frame, prefix, &values);
        }
    }
    for shape in data["BlendShapes"]
        .as_array()
        .ok_or("Missing BlendShapes")?
    {
        if let Some(name) = shape["k"].as_str().and_then(canonical) {
            frame.values.insert(name, number(&shape["v"])?);
        }
    }
    Ok(frame)
}
/// iFacialMocap percentages become 0..1 weights. Angles remain degrees.
/// Its documented stream has no face-presence flag; a valid pose implies presence.
pub fn parse_ifacial(bytes: &[u8]) -> Result<TrackingFrame, String> {
    if bytes.len() > MAX_FRAME {
        return Err("Tracking packet too large".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    let mut frame = TrackingFrame::default();
    let mut found = false;
    for field in text.split('|').map(str::trim).filter(|v| !v.is_empty()) {
        if let Some((key, raw)) = field.split_once('#') {
            if !matches!(
                key.trim_start_matches('=').trim(),
                "head" | "leftEye" | "rightEye"
            ) {
                continue;
            }
            let values = raw
                .split(',')
                .map(|s| {
                    s.trim()
                        .parse::<f64>()
                        .ok()
                        .filter(|v| v.is_finite())
                        .ok_or("Invalid pose number".to_owned())
                })
                .collect::<Result<Vec<_>, _>>()?;
            match key.trim_start_matches('=').trim() {
                "head" if values.len() == 6 => {
                    vector(&mut frame, "HeadRot", &values[..3]);
                    vector(&mut frame, "HeadPos", &values[3..]);
                    found = true;
                }
                "leftEye" if values.len() == 3 => vector(&mut frame, "EyeLeftRot", &values),
                "rightEye" if values.len() == 3 => vector(&mut frame, "EyeRightRot", &values),
                "head" | "leftEye" | "rightEye" => return Err("Invalid pose vector length".into()),
                _ => {}
            }
        } else if let Some((name, raw)) = field.split_once('&').or_else(|| field.split_once('-')) {
            if let Some(name) = canonical(name.trim()) {
                let value = raw
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .ok_or("Invalid blendshape number")?;
                frame.values.insert(name, value / 100.0);
                found = true;
            }
        }
    }
    if !found {
        return Err("No tracking measurements".into());
    }
    frame.face_present = true;
    frame.values.insert("FaceFound".into(), 1.0);
    Ok(frame)
}
#[derive(Default)]
struct Frames {
    buffer: Vec<u8>,
    discarding: bool,
}
impl Frames {
    fn push(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        const END: &[u8] = b"___iFacialMocap";
        let mut frames = Vec::new();
        for b in bytes {
            self.buffer.push(*b);
            if self.buffer.ends_with(END) {
                self.buffer.truncate(self.buffer.len() - END.len());
                if !self.discarding {
                    frames.push(std::mem::take(&mut self.buffer));
                }
                self.buffer.clear();
                self.discarding = false;
            } else if self.buffer.len() > MAX_FRAME {
                self.discarding = true;
                self.buffer.drain(..self.buffer.len() - END.len());
            }
        }
        frames
    }
}
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
        let request=serde_json::to_vec(&json!({"messageType":"iOSTrackingDataRequest","time":5.0,"sentBy":"SnenkBridge","ports":[udp.local_addr().map_err(|e|e.to_string())?.port()]})).unwrap();
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
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_and_coalesced_frames() {
        let data = b"jawOpen&50|=head#1,2,3,4,5,6|___iFacialMocapjawOpen-25|___iFacialMocap";
        for split in 0..=data.len() {
            let mut f = Frames::default();
            let mut frames = f.push(&data[..split]);
            frames.extend(f.push(&data[split..]));
            assert_eq!(frames.len(), 2);
            assert_eq!(parse_ifacial(&frames[0]).unwrap().values["JawOpen"], 0.5);
            assert_eq!(parse_ifacial(&frames[1]).unwrap().values["JawOpen"], 0.25);
        }
    }
    #[test]
    fn malformed_and_oversized_recover() {
        let mut f = Frames::default();
        assert!(f.push(&vec![b'x'; MAX_FRAME * 2]).is_empty());
        let frames = f.push(b"___iFacialMocapjawOpen&20|___iFacialMocap");
        assert_eq!(frames.len(), 1);
        assert!(parse_ifacial(b"jawOpen&NaN|").is_err());
        assert!(parse_ifacial(b"=head#1,2|").is_err());
        assert!(parse_ifacial(b"garbage").is_err());
    }
    #[test]
    fn normalized_sources_match() {
        let v=parse_vts_tracking(br#"{"FaceFound":true,"BlendShapes":[{"k":"JawOpen","v":0.5}],"Rotation":{"x":1,"y":2,"z":3},"future":123}"#).unwrap();
        let i = parse_ifacial(b"jawOpen&50|=head#1,2,3,0,0,0|").unwrap();
        for name in ["JawOpen", "HeadRotX", "HeadRotY", "HeadRotZ", "FaceFound"] {
            assert_eq!(v.values[name], i.values[name]);
        }
    }
    #[test]
    fn tcp_source_receives_fragmented_pose_and_releases_listener() {
        use std::{io::Write, net::TcpStream};
        let phone = UdpSocket::bind("127.0.0.1:49983").unwrap();
        phone
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut handle = SourceHandle::start(SourceSettings {
            kind: SourceKind::IFacialMocap,
            phone_address: "127.0.0.1".into(),
        });
        let mut buffer = [0; 512];
        let (n, _) = phone.recv_from(&mut buffer).unwrap();
        assert!(std::str::from_utf8(&buffer[..n])
            .unwrap()
            .starts_with("iFacialMocap_UDPTCP_"));
        let mut stream = TcpStream::connect("127.0.0.1:49986").unwrap();
        stream
            .write_all(b"jawOpen&bad|___iFacialMocapjawOpen&75|=head#1,2,")
            .unwrap();
        thread::sleep(Duration::from_millis(80));
        stream.write_all(b"3,4,5,6|___iFacialMocap").unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while handle.snapshot().2 == 0 && Instant::now() < deadline {
            thread::sleep(POLL);
        }
        let (status, frame, count) = handle.snapshot();
        assert_eq!(status, NetworkStatus::Connected);
        assert_eq!(count, 1);
        assert_eq!(frame.unwrap().values["JawOpen"], 0.75);
        let now = Instant::now();
        handle.stop();
        assert!(now.elapsed() < Duration::from_millis(500));
        let (n, _) = phone.recv_from(&mut buffer).unwrap();
        assert!(std::str::from_utf8(&buffer[..n])
            .unwrap()
            .starts_with("iFacialMocap_UDPTCPSTOP_"));
        drop(stream);
        let mut second = SourceHandle::start(SourceSettings {
            kind: SourceKind::IFacialMocap,
            phone_address: "127.0.0.1".into(),
        });
        phone.recv_from(&mut buffer).unwrap();
        second.stop();
        assert_eq!(second.snapshot().0, NetworkStatus::Stopped);
    }
    #[test]
    fn idle_source_cancels_and_restarts() {
        for _ in 0..2 {
            let mut h = SourceHandle::start(SourceSettings {
                kind: SourceKind::VTubeStudio,
                phone_address: "127.0.0.1".into(),
            });
            thread::sleep(Duration::from_millis(60));
            let now = Instant::now();
            h.stop();
            assert!(now.elapsed() < Duration::from_millis(500));
            assert_eq!(h.snapshot().0, NetworkStatus::Stopped);
        }
    }
}
