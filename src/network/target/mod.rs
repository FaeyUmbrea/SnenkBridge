mod api;
mod discovery;
mod token;

#[cfg(test)]
mod tests;

pub use discovery::{DiscoveredTarget, DiscoveryHandle};
pub use token::load_token;

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde_json::json;

use super::{cancelled, pause, set_status, NetworkStatus, Status, TargetSettings};
use crate::model::Parameter;

use api::Api;
#[cfg(test)]
pub(crate) use discovery::decode_discovery;
use token::{identity, save_token};

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
pub struct Failure {
    pub message: String,
    pub fatal: bool,
    pub code: Option<u64>,
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

pub fn fatal(message: impl Into<String>) -> Failure {
    Failure {
        message: message.into(),
        fatal: true,
        code: None,
    }
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
        api.call(
            "ParameterCreationRequest",
            json!({
                "parameterName": param.name,
                "explanation": "SnenkBridge output",
                "min": param.min,
                "max": param.max,
                "defaultValue": param.default_value
            }),
            stop,
        )?;
    }
    set_status(status, NetworkStatus::Connected);
    let mut heartbeat = Instant::now();
    while !cancelled(stop) {
        let output = latest.lock().unwrap().clone();
        if let Some((values, face)) = output {
            let parameters: Vec<_> = values
                .into_iter()
                .filter(|(k, _)| names.contains(k))
                .map(|(k, v)| json!({"id": k, "value": v}))
                .collect();
            api.call(
                "InjectParameterDataRequest",
                json!({
                    "faceFound": face,
                    "mode": "set",
                    "parameterValues": parameters
                }),
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
