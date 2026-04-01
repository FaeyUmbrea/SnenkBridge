use std::{
    collections::{HashSet, VecDeque},
    fs,
    net::{TcpStream, UdpSocket},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Receiver,
        Arc, LazyLock, Mutex,
    },
    time::{Duration, Instant},
};

use evalexpr::{
    Context, ContextWithMutableVariables, HashMapContext, IterateVariablesContext, Node,
};
use log::{error, info, warn};
use regex::Regex;
use serde_json::Value;
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

use crate::{
    tracking::response::TrackingResponse,
    utils::{get_current_timestamp, get_current_timestamp_ms},
    vts::{requests, responses},
};

type PrecalcCfg = (Vec<(String, String, Node)>, HashSet<u64>, VecDeque<Message>);

#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct VTSApiResponse<T> {
    api_name: String,
    api_version: String,
    timestamp: u64,
    message_type: String,
    #[serde(rename(deserialize = "requestID"))]
    request_id: String,
    data: T,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct VTSApiRequest<'a, T> {
    api_name: &'a str,
    api_version: &'a str,
    #[serde(rename(deserialize = "requestID"))]
    request_id: &'a str,
    message_type: &'a str,
    data: Option<T>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct CalcFn {
    name: String,
    func: String,
    min: f64,
    max: f64,
    default_value: f64,
}

pub struct VTubeStudioPlugin {
    receiver: Receiver<TrackingResponse>,
    transformation_cfg_path: String,
    config_reload_interval: Duration,
    face_search_timeout: u64,
    vts_ip: String,
    vts_port: String,

    last_context: LazyLock<Mutex<HashMapContext>>,
    last_context_timestamp: LazyLock<Mutex<u64>>,
}

impl VTubeStudioPlugin {
    const REQUEST_ID: &str = "SnenkBridge";
    const VTS_API_VERSION: &str = "1.0";
    const AFK_PARAMETERS: [&str; 3] = ["FaceFound", "Wave", "PingPong"];

    pub fn new(
        receiver: Receiver<TrackingResponse>,
        transformation_cfg_path: String,
        config_reload_delay: u64,
        face_search_timeout: u64,
        vts_ip: String,
        vts_port: String,
    ) -> Self {
        Self {
            receiver,
            transformation_cfg_path,
            config_reload_interval: Duration::from_millis(config_reload_delay),
            face_search_timeout,
            vts_ip,
            vts_port,
            last_context: LazyLock::new(|| Mutex::new(HashMapContext::new())),
            last_context_timestamp: LazyLock::new(|| Mutex::new(0)),
        }
    }

    pub fn run(&self, active: Arc<AtomicBool>) {
        while active.load(Ordering::Relaxed) {
            let flag = Arc::clone(&active);

            match self.connect(&active) {
                Some(websocket) => self.msg_loop(websocket, flag),
                None => break,
            }
        }
    }

    fn connect(&self, active: &Arc<AtomicBool>) -> Option<WebSocket<MaybeTlsStream<TcpStream>>> {
        let mut port = self.vts_port.clone();
        let ip = &self.vts_ip;
        loop {
            if !active.load(Ordering::Relaxed) {
                return None;
            }

            match tungstenite::connect(format!("ws://{}:{}", ip, port)) {
                Ok((websocket, _response)) => {
                    info!("Connected to VTS at {}:{}", ip, port);
                    return Some(websocket);
                }
                Err(error) => {
                    warn!("VTS connection failed: {}", error);
                    match VTubeStudioPlugin::discover_port() {
                        Ok(prt) => {
                            port = prt;
                        }
                        Err(e) => {
                            warn!("VTS discovery failed: {}", e);
                            std::thread::sleep(Duration::from_secs(2));
                            continue;
                        }
                    }
                }
            }
        }
    }

    fn discover_port() -> Result<String, String> {
        let mut buf = [0; 4096];

        let discovery_socket = match UdpSocket::bind("0.0.0.0:47779") {
            Ok(s) => s,
            Err(e) => return Err(e.to_string()),
        };

        match discovery_socket.set_read_timeout(Some(core::time::Duration::from_secs(3))) {
            Ok(m) => m,
            Err(e) => return Err(e.to_string()),
        };

        let (amt, _src) = match discovery_socket.recv_from(&mut buf) {
            Ok(m) => m,
            Err(e) => return Err(e.to_string()),
        };

        let data: VTSApiResponse<responses::Discovery> = match serde_json::from_slice(&buf[..amt]) {
            Ok(d) => d,
            Err(e) => return Err(e.to_string()),
        };

        Ok(data.data.port.to_string())
    }

    fn msg_loop(
        &self,
        mut websocket: WebSocket<MaybeTlsStream<TcpStream>>,
        active: Arc<AtomicBool>,
    ) {
        let mut msg_buffer: VecDeque<Message> = VecDeque::new();
        let token_path = if let Some(dir) = dirs::config_dir() {
            let app_dir = dir.join("SnenkBridge");
            let _ = fs::create_dir_all(&app_dir);
            app_dir.join("token")
        } else {
            std::path::PathBuf::from("token")
        };
        let mut token: Option<String> = fs::read_to_string(&token_path).ok();

        let vts_status = VTubeStudioPlugin::req_status_msg();
        let (mut precalc_funcs, mut used_timestamps, mut new_params) = self.precalc_cfg();

        msg_buffer.push_back(vts_status.clone());
        msg_buffer.append(&mut new_params);

        let mut last_time_config_reloaded = Instant::now();

        let mut dont_send = false;

        while active.load(Ordering::Relaxed) {
            if !self.config_reload_interval.is_zero()
                && last_time_config_reloaded.elapsed() > self.config_reload_interval
            {
                last_time_config_reloaded = Instant::now();

                (precalc_funcs, used_timestamps, new_params) = self.precalc_cfg();

                msg_buffer.clear();
                msg_buffer.push_back(vts_status.clone());
                msg_buffer.append(&mut new_params);

                info!("Config reloaded")
            }

            if !dont_send {
                if let Some(msg) = msg_buffer.front() {
                    match websocket.send(msg.clone()) {
                        Ok(_) => {}
                        Err(error) => {
                            warn!("Unable to send msg: {}", error);
                            break; // Reconnect
                        }
                    }
                } else if let Some(tracking_data) =
                    self.tracking_msg(&precalc_funcs, &used_timestamps)
                {
                    match websocket.send(tracking_data) {
                        Ok(_) => {}
                        Err(error) => {
                            warn!("Unable to send tracking msg: {}", error);
                            break; // Reconnect
                        }
                    }
                } else {
                    continue;
                }
            }

            match websocket.read() {
                Ok(msg) => {
                    if msg.is_text() {
                        let msg_text = match msg.to_text() {
                            Ok(t) => t,
                            Err(e) => {
                                warn!("Non-text message: {}", e);
                                continue;
                            }
                        };
                        let msg_value = match serde_json::from_str::<Value>(msg_text) {
                            Ok(v) => v,
                            Err(e) => {
                                warn!("Invalid JSON from VTS: {}", e);
                                continue;
                            }
                        };

                        match msg_value["messageType"].as_str() {
                            Some(msg_type) => match msg_type {
                                "APIError" => {
                                    let err_data = match serde_json::from_value::<
                                        VTSApiResponse<responses::APIError>,
                                    >(
                                        msg_value
                                    ) {
                                        Ok(d) => d,
                                        Err(e) => {
                                            warn!("Failed to parse API error: {}", e);
                                            continue;
                                        }
                                    };
                                    // warn!("API error: {:?}", err_data.data);
                                    match err_data.data.error_id {
                                        8 => {
                                            // msg_buffer.push_back(VtsPc::auth(&token));
                                        }
                                        51 => {
                                            // POPUP ON SCREEN

                                            // MAYBE
                                            // DELAY
                                            // msg_buffer.push_back(VtsPc::auth(&token));
                                        }
                                        352 => {
                                            // custom parameter exist
                                            msg_buffer.pop_front();
                                        }
                                        354 => {
                                            // custom parameter is default
                                            msg_buffer.pop_front();
                                        }
                                        450 => {
                                            //No param data was sended
                                        }
                                        _ => error!("Unknown API error: {:?}", err_data.data),
                                    }
                                }
                                "APIStateResponse" => {
                                    let state_data = match serde_json::from_value::<
                                        VTSApiResponse<responses::APIStateResponse>,
                                    >(
                                        msg_value
                                    ) {
                                        Ok(d) => d,
                                        Err(e) => {
                                            warn!("Failed to parse state response: {}", e);
                                            continue;
                                        }
                                    };
                                    msg_buffer.pop_front();
                                    if !state_data.data.current_session_authenticated {
                                        msg_buffer.push_front(VTubeStudioPlugin::auth(&token));
                                    }
                                }
                                "AuthenticationTokenResponse" => {
                                    let token_data = match serde_json::from_value::<
                                        VTSApiResponse<responses::AuthenticationToken>,
                                    >(
                                        msg_value
                                    ) {
                                        Ok(d) => d,
                                        Err(e) => {
                                            warn!("Failed to parse token response: {}", e);
                                            continue;
                                        }
                                    };

                                    let _ = fs::write(
                                        &token_path,
                                        &token_data.data.authentication_token,
                                    )
                                    .map_err(|e| error!("Unable to save token: {:?}", e));
                                    token = Some(token_data.data.authentication_token);
                                    info!("Recived Token from VtubeStudio");
                                    msg_buffer.pop_front();
                                    msg_buffer.push_front(VTubeStudioPlugin::auth(&token));
                                }
                                "AuthenticationResponse" => {
                                    let auth_data = match serde_json::from_value::<
                                        VTSApiResponse<responses::AuthenticationResponse>,
                                    >(
                                        msg_value
                                    ) {
                                        Ok(d) => d,
                                        Err(e) => {
                                            warn!("Failed to parse auth response: {}", e);
                                            continue;
                                        }
                                    };
                                    msg_buffer.pop_front();
                                    if !auth_data.data.authenticated {
                                        token = None;
                                        let _ = fs::remove_file(&token_path)
                                            .map_err(|e| error!("Unable to delete token: {:?}", e));
                                        info!("Invalid Token, Requesting new...");
                                        msg_buffer.push_back(VTubeStudioPlugin::auth(&token));
                                    }
                                }
                                "InjectParameterDataResponse" => {}
                                "ParameterCreationResponse" => {
                                    msg_buffer.pop_front();
                                }
                                _ => warn!("Unknown message: {}", msg_value["messageType"]),
                            },
                            None => warn!("No type in responce: {}", msg.to_text().unwrap()),
                        }
                        dont_send = false;
                    } else if msg.is_ping() || msg.is_pong() {
                        dont_send = true;
                        continue;
                    } else {
                        warn!("Non text response: {:?}", msg);
                        continue;
                    }
                }
                Err(error) => {
                    warn!("Unable to read msg: {}", error);
                    break; // Reconnect
                }
            }
        }
    }

    fn calculate_ppw(&self, total_milliseconds: u128, cycle_duration_ms: u64) -> (f64, f64) {
        let milliseconds_in_cycle = total_milliseconds as u64 % cycle_duration_ms;

        let milliseconds = milliseconds_in_cycle as f64;

        let ping_pong = milliseconds / cycle_duration_ms as f64;

        let half_cycle = (cycle_duration_ms as f64) / 2.0;
        let wave = if milliseconds < half_cycle {
            milliseconds / half_cycle
        } else {
            2.0 - (milliseconds / half_cycle)
        };

        (ping_pong, wave)
    }

    fn insert_cyclic_info(&self, context: &mut HashMapContext, used_timestamps: &HashSet<u64>) {
        let total_milliseconds = get_current_timestamp_ms();
        for v in used_timestamps {
            let (ping_pong, wave) = self.calculate_ppw(total_milliseconds, *v);
            context
                .set_value(format!("PingPong{v}"), ping_pong.into())
                .unwrap();
            context.set_value(format!("Wave{v}"), wave.into()).unwrap();
        }
    }

    fn track_cyclic_info_only(
        &self,
        precalc_funcs: &Vec<(String, String, Node)>,
        used_timestamps: &HashSet<u64>,
        face_search_timeout: &u64,
    ) -> Option<Message> {
        let mut params: Vec<requests::TrackingParam> = Vec::new();
        {
            let mut mutex_context = self.last_context.lock().unwrap();
            if mutex_context.iter_variables().len() == 0 {
                return None;
            }

            let mut face_found = mutex_context
                .get_value("FaceFound")
                .unwrap()
                .as_float()
                .unwrap();
            if face_found == 1.0 {
                let timestamp = self.last_context_timestamp.lock().unwrap();
                let difference = get_current_timestamp() as f64 - *timestamp as f64;
                let timeout = *face_search_timeout as f64;
                if difference > timeout {
                    face_found = 0.0;
                    mutex_context
                        .set_value("FaceFound".into(), face_found.into())
                        .unwrap();
                }
            }
            self.insert_cyclic_info(&mut mutex_context, used_timestamps);

            let cloned_context = mutex_context.clone();
            for (key, func, node) in precalc_funcs {
                for parameter in Self::AFK_PARAMETERS {
                    if func.contains(parameter) {
                        params.push(requests::TrackingParam {
                            id: key.as_str(),
                            value: node
                                .eval_with_context(&cloned_context)
                                .unwrap()
                                .as_float()
                                .unwrap()
                                .clamp(-1_000_000.0, 1_000_000.0),
                            weight: Some(1.0),
                        });
                        break;
                    }
                }
            }
        }

        let params_data = requests::InjectParams {
            face_found: false,
            mode: "set",
            parameter_values: params,
        };
        let message_type = "InjectParameterDataRequest";
        let request = VTSApiRequest {
            data: Some(params_data),
            api_name: "VTubeStudioPublicAPI",
            api_version: Self::VTS_API_VERSION,
            request_id: Self::REQUEST_ID,
            message_type,
        };

        let request_string = serde_json::to_string(&request).unwrap();
        Some(Message::text(request_string))
    }

    fn tracking_msg(
        &self,
        precalc_funcs: &Vec<(String, String, Node)>,
        used_timestamps: &HashSet<u64>,
    ) -> Option<Message> {
        let mut context = HashMapContext::new();

        let mut binding = self.receiver.try_iter();
        let it = binding.by_ref();

        let raw_data = match it.last() {
            Some(data) => data,
            None => {
                return self.track_cyclic_info_only(
                    precalc_funcs,
                    used_timestamps,
                    &self.face_search_timeout,
                );
            }
        };

        self.insert_cyclic_info(&mut context, used_timestamps);

        for v in &raw_data.blend_shapes {
            context.set_value(v.k.clone(), v.v.into()).unwrap();
        }

        context
            .set_value("HeadPosX".into(), raw_data.position.x.into())
            .unwrap();
        context
            .set_value("HeadPosY".into(), raw_data.position.y.into())
            .unwrap();
        context
            .set_value("HeadPosZ".into(), raw_data.position.z.into())
            .unwrap();

        context
            .set_value("HeadRotX".into(), raw_data.rotation.x.into())
            .unwrap();
        context
            .set_value("HeadRotY".into(), raw_data.rotation.y.into())
            .unwrap();
        context
            .set_value("HeadRotZ".into(), raw_data.rotation.z.into())
            .unwrap();

        context
            .set_value(
                "FaceFound".into(),
                (if raw_data.face_found { 1.0 } else { 0.0 }).into(),
            )
            .unwrap();

        let mut params: Vec<requests::TrackingParam> = Vec::new();

        if raw_data.face_found {
            for (key, _, node) in precalc_funcs {
                params.push(requests::TrackingParam {
                    id: key.as_str(),
                    value: node
                        .eval_with_context(&context)
                        .unwrap()
                        .as_float()
                        .unwrap()
                        .clamp(-1000000.0, 1000000.0),
                    weight: Some(1.0),
                });
            }
        }

        if params.is_empty() {
            return None;
        }

        {
            let mut mutex_context = self.last_context.lock().unwrap();
            *mutex_context = context;

            let mut timestamp = self.last_context_timestamp.lock().unwrap();
            *timestamp = get_current_timestamp();
        }

        let params_data = requests::InjectParams {
            face_found: raw_data.face_found,
            mode: "set",
            parameter_values: params,
        };
        let message_type = "InjectParameterDataRequest";
        let request = VTSApiRequest {
            data: Some(params_data),
            api_name: "VTubeStudioPublicAPI",
            api_version: Self::VTS_API_VERSION,
            request_id: Self::REQUEST_ID,
            message_type,
        };

        let request_string = serde_json::to_string(&request).unwrap();
        Some(Message::text(request_string))
    }

    fn req_status_msg() -> Message {
        let status_req = VTSApiRequest::<i32> {
            data: None,
            api_name: "VTubeStudioPublicAPI",
            api_version: Self::VTS_API_VERSION,
            request_id: Self::REQUEST_ID,
            message_type: "APIStateRequest",
        };

        let status_req_msg = serde_json::to_string(&status_req).unwrap();
        info!("Requesing status of VtubeStudio");
        Message::text(status_req_msg)
    }

    fn auth(token: &Option<String>) -> Message {
        if token.is_some() {
            let tk = token.clone().unwrap();

            let auth_token = requests::Auth {
                plugin_name: "SnenkBridge",
                plugin_developer: "FaeyUmbrea",
                authentication_token: tk.as_str(),
            };

            let auth_req = VTSApiRequest {
                data: Some(auth_token),
                api_name: "VTubeStudioPublicAPI",
                api_version: Self::VTS_API_VERSION,
                request_id: Self::REQUEST_ID,
                message_type: "AuthenticationRequest",
            };

            let auth_req_msg = serde_json::to_string(&auth_req).unwrap();

            info!("Authentication Request to VtubeStudio");
            return Message::text(auth_req_msg);
        }

        let auth_data = requests::AuthToken {
            plugin_name: "SnenkBridge",
            plugin_developer: "FaeyUmbrea",
            plugin_icon: None,
        };

        let token_req = VTSApiRequest {
            data: Some(auth_data),
            api_name: "VTubeStudioPublicAPI",
            api_version: Self::VTS_API_VERSION,
            request_id: Self::REQUEST_ID,
            message_type: "AuthenticationTokenRequest",
        };

        let token_req_msg = serde_json::to_string(&token_req).unwrap();

        info!("Authentication Token Request: Please accept PopUp in VtubeStudio");
        Message::text(token_req_msg)
    }

    fn extract_wave_pingpong_numbers(&self, input: &str) -> HashSet<u64> {
        let re = Regex::new(r"(Wave|PingPong)(\d+)").unwrap();

        re.captures_iter(input)
            .filter_map(|caps| caps.get(2)?.as_str().parse::<u64>().ok())
            .collect()
    }

    fn precalc_cfg(&self) -> PrecalcCfg {
        info!(
            "Loadling tranformation config: {}",
            &self.transformation_cfg_path
        );

        let def_params = [
            String::from("FacePositionX"),
            String::from("FacePositionY"),
            String::from("FacePositionZ"),
            String::from("FaceAngleX"),
            String::from("FaceAngleY"),
            String::from("FaceAngleZ"),
            String::from("MouthSmile"),
            String::from("MouthOpen"),
            String::from("Brows"),
            String::from("TongueOut"),
            String::from("EyeOpenLeft"),
            String::from("EyeOpenRight"),
            String::from("EyeLeftX"),
            String::from("EyeLeftY"),
            String::from("EyeRightX"),
            String::from("EyeRightY"),
            String::from("CheekPuff"),
            String::from("FaceAngry"),
            String::from("BrowLeftY"),
            String::from("BrowRightY"),
            String::from("MouthX"),
            String::from("VoiceFrequencyPlusMouthSmile"),
        ];

        let mut new_params: VecDeque<Message> = VecDeque::new();
        let config = match fs::read_to_string(&self.transformation_cfg_path) {
            Ok(content) => content,
            Err(error) => {
                warn!(
                    "Unable to load transformation config '{}': {}",
                    &self.transformation_cfg_path, error
                );
                return (Vec::new(), HashSet::new(), new_params);
            }
        };

        let calc_fns: Vec<CalcFn> = match serde_json::from_str(&config[..]) {
            Ok(value) => value,
            Err(error) => {
                warn!(
                    "Unable to parse transformation config '{}': {}",
                    &self.transformation_cfg_path, error
                );
                return (Vec::new(), HashSet::new(), new_params);
            }
        };

        let mut timestamps = HashSet::new();
        let mut precalc_fns: Vec<_> = Vec::new();
        for func in calc_fns.into_iter() {
            let name: String = func.name;

            info!("Loading parameter: {}", &name);
            if !def_params.contains(&name) {
                let param_data = requests::ParameterCreation {
                    parameter_name: name.clone(),
                    explanation: "Custom Snenk Bridge param".to_string(),
                    min: func.min,
                    max: func.max,
                    default_value: func.default_value,
                };

                let param_req = VTSApiRequest {
                    data: Some(param_data),
                    api_name: "VTubeStudioPublicAPI",
                    api_version: Self::VTS_API_VERSION,
                    request_id: Self::REQUEST_ID,
                    message_type: "ParameterCreationRequest",
                };

                let param_req_msg = serde_json::to_string(&param_req).unwrap();

                new_params.push_back(Message::text(param_req_msg));
            }

            let local_timestamps = self.extract_wave_pingpong_numbers(&func.func);
            timestamps = timestamps.union(&local_timestamps).cloned().collect();

            let node = match evalexpr::build_operator_tree(&func.func[..]) {
                Ok(calc) => calc,
                Err(error) => {
                    error!(
                        "Skipping parameter '{}': invalid expression '{}': {}",
                        name, func.func, error
                    );
                    continue;
                }
            };

            precalc_fns.push((name, func.func.clone(), node));
        }

        info!("Tranformation config loaded");
        (precalc_fns, timestamps, new_params)
    }
}
