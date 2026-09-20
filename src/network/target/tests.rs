use std::{
    collections::BTreeMap,
    fs,
    net::TcpListener,
    path::PathBuf,
    sync::{atomic::AtomicBool, mpsc},
    thread,
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use tungstenite::{Message, WebSocket};

use super::{
    decode_discovery, load_token, Api, NetworkStatus, Parameter, TargetHandle, TargetSettings,
};

fn request(ws: &mut WebSocket<std::net::TcpStream>) -> Value {
    serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap()
}

fn reply(ws: &mut WebSocket<std::net::TcpStream>, req: &Value, data: Value) {
    let kind = req["messageType"]
        .as_str()
        .unwrap()
        .replace("Request", "Response");
    ws.send(Message::text(
        json!({"messageType":kind,"requestID":req["requestID"],"data":data}).to_string(),
    ))
    .unwrap();
}

fn settings(port: u16, path: PathBuf) -> TargetSettings {
    TargetSettings {
        host: "127.0.0.1".into(),
        port,
        token_path: path,
    }
}

fn parameter(name: &str) -> Parameter {
    Parameter {
        name: name.into(),
        func: "1".into(),
        min: -2.0,
        max: 2.0,
        default_value: 0.5,
        delay_buffer: None,
    }
}

#[test]
fn reauthorizes_registers_and_reconnects_with_latest_output() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("token");
    fs::write(&path, "stale-token").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    let server = thread::spawn(move || {
        for attempt in 0..2 {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            let r = request(&mut ws);
            assert_eq!(r["messageType"], "APIStateRequest");
            reply(&mut ws, &r, json!({"active":true}));
            let r = request(&mut ws);
            assert_eq!(r["messageType"], "AuthenticationRequest");
            assert_eq!(r["data"]["pluginDeveloper"], "FaeyUmbrea");
            if attempt == 0 {
                assert_eq!(r["data"]["authenticationToken"], "stale-token");
                reply(&mut ws, &r, json!({"authenticated":false}));
                let r = request(&mut ws);
                assert_eq!(r["messageType"], "AuthenticationTokenRequest");
                reply(&mut ws, &r, json!({"authenticationToken":"fresh-token"}));
                let r = request(&mut ws);
                assert_eq!(r["data"]["authenticationToken"], "fresh-token");
                reply(&mut ws, &r, json!({"authenticated":true}));
            } else {
                assert_eq!(r["data"]["authenticationToken"], "fresh-token");
                reply(&mut ws, &r, json!({"authenticated":true}));
            }
            let r = request(&mut ws);
            assert_eq!(r["messageType"], "InputParameterListRequest");
            reply(
                &mut ws,
                &r,
                json!({"defaultParameters":[{"name":"FaceAngleX"}],"customParameters":[{"name":"OtherParam","addedBy":"Someone"}]}),
            );
            let r = request(&mut ws);
            assert_eq!(r["messageType"], "ParameterCreationRequest");
            assert_eq!(r["data"]["parameterName"], "NewParam");
            assert_eq!(r["data"]["defaultValue"], 0.5);
            reply(&mut ws, &r, json!({}));
            let r = request(&mut ws);
            assert_eq!(r["messageType"], "InjectParameterDataRequest");
            reply(&mut ws, &r, json!({}));
            tx.send(r["data"].clone()).unwrap();
        }
    });
    let mut handle = TargetHandle::start(
        settings(port, path.clone()),
        vec![
            parameter("FaceAngleX"),
            parameter("OtherParam"),
            parameter("NewParam"),
        ],
    );
    handle.publish(BTreeMap::from([("NewParam".into(), 1.0)]), true);
    let first = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(first["parameterValues"][0]["value"], 1.0);
    handle.publish(
        BTreeMap::from([
            ("NewParam".into(), 9_000_000.0),
            ("OtherParam".into(), f64::NAN),
        ]),
        false,
    );
    let second = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(second["parameterValues"][0]["value"], 1_000_000.0);
    assert_eq!(second["parameterValues"].as_array().unwrap().len(), 1);
    assert_eq!(second["faceFound"], false);
    handle.stop();
    server.join().unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), "fresh-token");
}

#[test]
fn partial_websocket_response_survives_read_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let r = request(&mut ws);
        let payload = json!({"messageType":"APIStateResponse","requestID":r["requestID"],"data":{"active":true}}).to_string();
        assert!(payload.len() < 126);
        let mut wire = vec![0x81, payload.len() as u8];
        wire.extend_from_slice(payload.as_bytes());
        std::io::Write::write_all(ws.get_mut(), &wire[..5]).unwrap();
        thread::sleep(Duration::from_millis(150));
        std::io::Write::write_all(ws.get_mut(), &wire[5..]).unwrap();
    });
    let stop = AtomicBool::new(false);
    let mut api = Api::connect(&settings(port, PathBuf::new()), &stop).unwrap();
    assert_eq!(
        api.call("APIStateRequest", json!({}), &stop).unwrap()["active"],
        true
    );
    server.join().unwrap();
}

#[test]
fn pending_authorization_can_be_cancelled() {
    let temp = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let r = request(&mut ws);
        reply(&mut ws, &r, json!({"active":true}));
        let r = request(&mut ws);
        assert_eq!(r["messageType"], "AuthenticationTokenRequest");
        tx.send(()).unwrap();
        let _ = ws.read();
    });
    let mut handle = TargetHandle::start(settings(port, temp.path().join("token")), vec![]);
    rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(handle.status(), NetworkStatus::Authorizing);
    let now = Instant::now();
    handle.stop();
    assert!(now.elapsed() < Duration::from_millis(500));
    server.join().unwrap();
}

#[test]
fn malformed_tokens_are_replaced_and_discovery_preserves_sender() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("token");
    fs::write(&path, "x".repeat(65)).unwrap();
    assert_eq!(load_token(&path).unwrap(), None);
    let target = decode_discovery(
        br#"{"apiName":"VTubeStudioPublicAPI","messageType":"VTubeStudioAPIStateBroadcast","data":{"port":8999,"active":true,"instanceID":"test","windowTitle":"Test"}}"#,
        "192.0.2.10".parse().unwrap(),
    )
    .unwrap();
    assert_eq!(target.host, "192.0.2.10");
    assert_eq!(target.port, 8999);
    assert!(decode_discovery(b"{}", "127.0.0.1".parse().unwrap()).is_none());
}
