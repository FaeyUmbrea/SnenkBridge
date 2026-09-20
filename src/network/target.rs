use super::*;
use crate::model::Parameter;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{TcpStream, UdpSocket},
    thread::JoinHandle,
};
use tungstenite::{handshake::HandshakeError, protocol::WebSocketConfig, Message, WebSocket};

type Output = Option<(BTreeMap<String, f64>, bool)>;
pub struct TargetHandle {
    stop: Arc<AtomicBool>,
    status: Status,
    latest: Arc<Mutex<Output>>,
    worker: Option<JoinHandle<()>>,
}
impl TargetHandle {
    pub fn start(settings: TargetSettings, params: Vec<Parameter>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(NetworkStatus::Connecting));
        let latest = Arc::new(Mutex::new(None));
        let (s, st, values) = (stop.clone(), status.clone(), latest.clone());
        let worker = thread::spawn(move || {
            let mut token = match load_token(&settings.token_path) {
                Ok(t) => t,
                Err(e) => {
                    set_status(&st, NetworkStatus::Error(e));
                    return;
                }
            };
            while !cancelled(&s) {
                set_status(&st, NetworkStatus::Connecting);
                match session(&settings, &params, &s, &st, &values, &mut token) {
                    Ok(()) => {}
                    Err(e) => {
                        set_status(&st, NetworkStatus::Error(e.message));
                        if e.fatal {
                            return;
                        }
                    }
                }
                pause(&s, Duration::from_secs(1));
            }
            set_status(&st, NetworkStatus::Stopped);
        });
        Self {
            stop,
            status,
            latest,
            worker: Some(worker),
        }
    }
    pub fn publish(&self, mut values: BTreeMap<String, f64>, face_present: bool) {
        values.retain(|_, v| v.is_finite());
        for v in values.values_mut() {
            *v = v.clamp(-1_000_000.0, 1_000_000.0);
        }
        *self.latest.lock().unwrap() = Some((values, face_present));
    }
    pub fn status(&self) -> NetworkStatus {
        self.status.lock().unwrap().clone()
    }
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.worker.take() {
            let _ = t.join();
        }
        set_status(&self.status, NetworkStatus::Stopped);
    }
}
impl Drop for TargetHandle {
    fn drop(&mut self) {
        self.stop();
    }
}
#[derive(Debug)]
struct Failure {
    message: String,
    fatal: bool,
    code: Option<u64>,
}
impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self {
            message,
            fatal: false,
            code: None,
        }
    }
}
impl From<std::io::Error> for Failure {
    fn from(e: std::io::Error) -> Self {
        e.to_string().into()
    }
}
fn fatal(message: impl Into<String>) -> Failure {
    Failure {
        message: message.into(),
        fatal: true,
        code: None,
    }
}
struct Api {
    socket: WebSocket<TcpStream>,
    sequence: u64,
}
impl Api {
    fn connect(settings: &TargetSettings, stop: &AtomicBool) -> Result<Self, Failure> {
        let addr = resolve(&settings.host, settings.port, stop)?;
        let stream=TcpStream::connect_timeout(&addr,Duration::from_millis(250)).or_else(|first| {
            match discover_port(addr.ip(),stop) { Some(port)=>TcpStream::connect_timeout(&SocketAddr::new(addr.ip(),port),Duration::from_millis(250)), None=>Err(first) }
        }).map_err(|e|format!("Cannot connect to VTube Studio: {e}. Check host, port and Allow Plugin API access."))?;
        stream.set_read_timeout(Some(POLL))?;
        stream.set_write_timeout(Some(POLL))?;
        stream.set_nodelay(true)?;
        stream.set_nonblocking(true)?;
        let port = stream.peer_addr()?.port();
        let host = if addr.is_ipv6() {
            format!("[{}]", addr.ip())
        } else {
            addr.ip().to_string()
        };
        let mut config = WebSocketConfig::default();
        config.max_message_size = Some(1024 * 1024);
        config.max_frame_size = Some(1024 * 1024);
        config.max_write_buffer_size = 2 * 1024 * 1024;
        let mut handshake = tungstenite::client::client_with_config(
            format!("ws://{host}:{port}/"),
            stream,
            Some(config),
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if cancelled(stop) || Instant::now() > deadline {
                return Err("WebSocket handshake cancelled or timed out"
                    .to_owned()
                    .into());
            }
            match handshake {
                Ok((socket, _)) => {
                    socket.get_ref().set_nonblocking(false)?;
                    return Ok(Self {
                        socket,
                        sequence: 0,
                    });
                }
                Err(HandshakeError::Interrupted(mid)) => {
                    pause(stop, POLL);
                    handshake = mid.handshake();
                }
                Err(HandshakeError::Failure(e)) => return Err(e.to_string().into()),
            }
        }
    }
    fn request(
        &mut self,
        kind: &str,
        data: Value,
        stop: &AtomicBool,
        timeout: Duration,
    ) -> Result<Value, Failure> {
        self.sequence += 1;
        let id = format!("snenk-{}", self.sequence);
        self.socket
            .send(Message::text(
                json!({"apiName":"VTubeStudioPublicAPI","apiVersion":"1.0","requestID":id,"messageType":kind,"data":data})
                    .to_string(),
            ))
            .map_err(|e| e.to_string())?;
        let deadline = Instant::now() + timeout;
        while !cancelled(stop) && Instant::now() < deadline {
            match self.socket.read() {
                Ok(Message::Text(text)) => {
                    let response: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
                    if response["requestID"] != id {
                        continue;
                    }
                    if response["messageType"] == "APIError" {
                        let code = response["data"]["errorID"].as_u64().unwrap_or(0);
                        let message = format!(
                            "VTube Studio API error {code}: {}",
                            response["data"]["message"]
                                .as_str()
                                .unwrap_or("Unknown error")
                        );
                        return Err(Failure {
                            message,
                            fatal: code == 50,
                            code: Some(code),
                        });
                    }
                    let expected =
                        kind.strip_suffix("Request").unwrap_or(kind).to_owned() + "Response";
                    if response["messageType"] != expected {
                        return Err("Unexpected VTube Studio response type".to_owned().into());
                    }
                    return Ok(response["data"].clone());
                }
                Ok(Message::Close(_)) => {
                    return Err("VTube Studio closed the connection".to_owned().into())
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if transient(&e) => {}
                Err(e) => return Err(e.to_string().into()),
            }
        }
        Err("VTube Studio request cancelled or timed out"
            .to_owned()
            .into())
    }
    fn call(&mut self, kind: &str, data: Value, stop: &AtomicBool) -> Result<Value, Failure> {
        self.request(kind, data, stop, Duration::from_secs(3))
    }
}
fn identity() -> Value {
    json!({"pluginName":"SnenkBridge","pluginDeveloper":"FaeyUmbrea"})
}
fn load_token(path: &std::path::Path) -> Result<Option<String>, String> {
    match fs::File::open(path) {
        Ok(file) => {
            let mut token = String::new();
            file.take(65)
                .read_to_string(&mut token)
                .map_err(|e| format!("Cannot read authorization token: {e}"))?;
            if token.is_empty() || token.len() > 64 || !token.is_ascii() {
                return Ok(None);
            }
            Ok(Some(token))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Cannot read authorization token: {e}")),
    }
}
fn save_token(path: &std::path::Path, token: &str) -> Result<(), Failure> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| fatal(format!("Cannot create token directory: {e}")))?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let temp = path.with_extension(format!(
        "token-{}-{}.tmp",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| -> std::io::Result<()> {
        let mut file = options.open(&temp)?;
        file.write_all(token.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(|e| fatal(format!("Cannot save authorization token: {e}")))
}
fn session(
    settings: &TargetSettings,
    params: &[Parameter],
    stop: &AtomicBool,
    status: &Status,
    latest: &Mutex<Output>,
    token: &mut Option<String>,
) -> Result<(), Failure> {
    let mut api = Api::connect(settings, stop)?;
    let state = api.call("APIStateRequest", json!({}), stop)?;
    if state["active"] != true {
        return Err(fatal(
            "VTube Studio API access is disabled. Enable Allow Plugin API access.",
        ));
    }
    set_status(status, NetworkStatus::Authorizing);
    let mut authenticated = false;
    if let Some(token) = token.as_ref() {
        let mut data = identity();
        data["authenticationToken"] = json!(token);
        match api.call("AuthenticationRequest", data, stop) {
            Ok(response) => authenticated = response["authenticated"] == true,
            Err(e) if e.code == Some(50) => {}
            Err(e) => return Err(e),
        }
    }
    if !authenticated {
        let response = api.request(
            "AuthenticationTokenRequest",
            identity(),
            stop,
            Duration::from_secs(120),
        )?;
        let new_token = response["authenticationToken"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 64 && s.is_ascii())
            .ok_or_else(|| fatal("Invalid authorization token response"))?
            .to_owned();
        let mut data = identity();
        data["authenticationToken"] = json!(new_token);
        let response = api.call("AuthenticationRequest", data, stop)?;
        if response["authenticated"] != true {
            return Err(fatal(format!(
                "VTube Studio authentication rejected: {}",
                response["reason"].as_str().unwrap_or("Unknown reason")
            )));
        }
        save_token(&settings.token_path, &new_token)?;
        *token = Some(new_token);
    }
    let list = api.call("InputParameterListRequest", json!({}), stop)?;
    let defaults = list["defaultParameters"]
        .as_array()
        .ok_or_else(|| fatal("Invalid VTube Studio parameter list"))?;
    let customs = list["customParameters"]
        .as_array()
        .ok_or_else(|| fatal("Invalid VTube Studio custom parameter list"))?;
    let mut names = BTreeSet::new();
    for param in params {
        if !names.insert(&param.name) {
            continue;
        }
        if defaults.iter().any(|p| p["name"] == param.name) {
            continue;
        }
        if customs
            .iter()
            .any(|p| p["name"] == param.name && p["addedBy"] != "SnenkBridge")
        {
            continue;
        }
        if !(4..=32).contains(&param.name.len())
            || !param.name.chars().all(|c| c.is_ascii_alphanumeric())
        {
            return Err(fatal(format!(
                "Invalid custom parameter name: {}",
                param.name
            )));
        }
        if [param.min, param.max, param.default_value]
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
            || param.min > param.max
        {
            return Err(fatal(format!("Invalid range for {}", param.name)));
        }
        api.call("ParameterCreationRequest",json!({"parameterName":param.name,"explanation":"SnenkBridge output","min":param.min,"max":param.max,"defaultValue":param.default_value}),stop)?;
    }
    set_status(status, NetworkStatus::Connected);
    let mut heartbeat = Instant::now();
    while !cancelled(stop) {
        let output = latest.lock().unwrap().clone();
        if let Some((values, face)) = output {
            let parameters: Vec<_> = values
                .into_iter()
                .filter(|(k, _)| names.contains(k))
                .map(|(k, v)| json!({"id":k,"value":v}))
                .collect();
            api.call(
                "InjectParameterDataRequest",
                json!({"faceFound":face,"mode":"set","parameterValues":parameters}),
                stop,
            )?;
            heartbeat = Instant::now();
        } else if heartbeat.elapsed() >= Duration::from_secs(1) {
            api.call("APIStateRequest", json!({}), stop)?;
            heartbeat = Instant::now();
        }
        pause(stop, Duration::from_millis(16));
    }
    Ok(())
}

fn decode_discovery(bytes: &[u8], host: IpAddr) -> Option<DiscoveredTarget> {
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
// Discovery can change the port, never the host chosen by the user.
fn discover_port(host: IpAddr, stop: &AtomicBool) -> Option<u16> {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredTarget {
    pub host: String,
    pub port: u16,
    pub active: bool,
    pub instance_id: String,
    pub window_title: String,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, sync::mpsc};
    fn request(ws: &mut WebSocket<TcpStream>) -> Value {
        serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap()
    }
    fn reply(ws: &mut WebSocket<TcpStream>, req: &Value, data: Value) {
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
            let payload=json!({"messageType":"APIStateResponse","requestID":r["requestID"],"data":{"active":true}}).to_string();
            assert!(payload.len() < 126);
            let mut wire = vec![0x81, payload.len() as u8];
            wire.extend_from_slice(payload.as_bytes());
            ws.get_mut().write_all(&wire[..5]).unwrap();
            thread::sleep(Duration::from_millis(150));
            ws.get_mut().write_all(&wire[5..]).unwrap();
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
        let target=decode_discovery(br#"{"apiName":"VTubeStudioPublicAPI","messageType":"VTubeStudioAPIStateBroadcast","data":{"port":8999,"active":true,"instanceID":"test","windowTitle":"Test"}}"#,"192.0.2.10".parse().unwrap()).unwrap();
        assert_eq!(target.host, "192.0.2.10");
        assert_eq!(target.port, 8999);
        assert!(decode_discovery(b"{}", "127.0.0.1".parse().unwrap()).is_none());
    }
}
