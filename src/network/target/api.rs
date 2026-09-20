use std::{
    net::TcpStream,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use tungstenite::{handshake::HandshakeError, Message, WebSocket};

use crate::network::{cancelled, resolve, transient, TargetSettings, POLL};

use super::{discovery::discover_port, fatal, Failure};

pub struct Api {
    pub socket: WebSocket<TcpStream>,
    pub sequence: u64,
}

impl Api {
    pub fn connect(settings: &TargetSettings, stop: &AtomicBool) -> Result<Self, Failure> {
        let mut port = settings.port;
        let mut addr = resolve(&settings.host, port, stop)?;
        if port == 0 {
            port = discover_port(addr.ip(), stop)
                .ok_or_else(|| fatal("Cannot discover VTube Studio port"))?;
            addr = resolve(&settings.host, port, stop)?;
        }
        let stream = match TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
            Ok(s) => s,
            Err(e) => {
                let found = discover_port(addr.ip(), stop);
                if let Some(p) = found.filter(|p| *p != port) {
                    let next = resolve(&settings.host, p, stop)?;
                    TcpStream::connect_timeout(&next, Duration::from_secs(2)).map_err(|e| {
                        fatal(format!(
                            "Cannot connect to VTube Studio on {}:{} (discovered {}): {e}",
                            settings.host, port, p
                        ))
                    })?
                } else {
                    return Err(fatal(format!(
                        "Cannot connect to VTube Studio on {}:{}: {e}",
                        settings.host, port
                    )));
                }
            }
        };
        stream
            .set_read_timeout(Some(POLL))
            .map_err(|e| fatal(e.to_string()))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .map_err(|e| fatal(e.to_string()))?;
        let url = format!("ws://{}:{}", settings.host, port);
        let mut builder = tungstenite::client(url, stream);
        let deadline = Instant::now() + Duration::from_secs(3);
        let socket = loop {
            if cancelled(stop) {
                return Err("Connection cancelled".to_owned().into());
            }
            match builder {
                Ok((socket, _)) => break socket,
                Err(HandshakeError::Interrupted(mid)) => {
                    if Instant::now() >= deadline {
                        return Err(fatal("VTube Studio handshake timed out"));
                    }
                    builder = mid.handshake();
                }
                Err(HandshakeError::Failure(e)) => {
                    return Err(fatal(format!("VTube Studio handshake failed: {e}")));
                }
            }
        };
        Ok(Self {
            socket,
            sequence: 0,
        })
    }

    pub fn request(
        &mut self,
        kind: &str,
        data: Value,
        stop: &AtomicBool,
        timeout: Duration,
    ) -> Result<Value, Failure> {
        self.sequence += 1;
        let id = format!("req-{}", self.sequence);
        let body = json!({
            "apiName": "VTubeStudioPublicAPI",
            "apiVersion": "1.0",
            "requestID": id,
            "messageType": kind,
            "data": data,
        });
        self.socket
            .send(Message::text(body.to_string()))
            .map_err(|e| e.to_string())?;
        let deadline = Instant::now() + timeout;
        while !cancelled(stop) && Instant::now() < deadline {
            match self.socket.read() {
                Ok(Message::Text(text)) => {
                    let response: Value = serde_json::from_str(&text)
                        .map_err(|e| format!("Invalid VTube Studio response: {e}"))?;
                    if response["requestID"] != id {
                        continue;
                    }
                    if response["messageType"] == "APIError" {
                        let code = response["data"]["errorID"].as_u64().unwrap_or(0);
                        let message = response["data"]["message"]
                            .as_str()
                            .unwrap_or("Unknown VTube Studio API error")
                            .to_owned();
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
                    return Err("VTube Studio closed the connection".to_owned().into());
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

    pub fn call(&mut self, kind: &str, data: Value, stop: &AtomicBool) -> Result<Value, Failure> {
        self.request(kind, data, stop, Duration::from_secs(3))
    }
}
