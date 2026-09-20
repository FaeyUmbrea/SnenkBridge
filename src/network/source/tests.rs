use std::{
    io::Write,
    net::{TcpListener, TcpStream, UdpSocket},
    time::Duration,
};

use crate::network::{SourceHandle, SourceKind, SourceSettings, MAX_FRAME};

use super::{ifacial::Frames, parse_ifacial, parse_vts_tracking};

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
    let v = parse_vts_tracking(br#"{"FaceFound":true,"BlendShapes":[{"k":"JawOpen","v":0.5}],"Rotation":{"x":1,"y":2,"z":3},"future":123}"#).unwrap();
    let i = parse_ifacial(b"jawOpen&50|=head#1,2,3,0,0,0|").unwrap();
    for name in ["JawOpen", "HeadRotX", "HeadRotY", "HeadRotZ", "FaceFound"] {
        assert_eq!(v.values[name], i.values[name]);
    }
}

#[test]
fn tcp_source_receives_fragmented_pose_and_releases_listener() {
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
        .write_all(b"=head#0,0,0,0,0,0|jawOpen&42|___iFacialMocap")
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let mut frame = None;
    while std::time::Instant::now() < deadline {
        let (_, latest, _) = handle.snapshot();
        if latest.is_some() {
            frame = latest;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(frame.unwrap().values["JawOpen"], 0.42);
    handle.stop();
    drop(stream);
    assert!(TcpListener::bind("127.0.0.1:49986").is_ok());
}

#[test]
fn idle_source_cancels_and_restarts() {
    let mut handle = SourceHandle::start(SourceSettings {
        kind: SourceKind::VTubeStudio,
        phone_address: "127.0.0.1".into(),
    });
    std::thread::sleep(Duration::from_millis(100));
    handle.stop();
    let mut handle2 = SourceHandle::start(SourceSettings {
        kind: SourceKind::VTubeStudio,
        phone_address: "127.0.0.1".into(),
    });
    std::thread::sleep(Duration::from_millis(100));
    handle2.stop();
}
