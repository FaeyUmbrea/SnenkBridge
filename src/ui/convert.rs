use std::{
    rc::Rc,
    time::{Duration, Instant},
};

use slint::{ModelRc, SharedString, VecModel};

use crate::model::{DelaySettings, Parameter};
use crate::network::NetworkStatus;
use crate::EditorParam;

pub fn strings(values: impl IntoIterator<Item = String>) -> ModelRc<SharedString> {
    Rc::new(VecModel::from(
        values.into_iter().map(Into::into).collect::<Vec<_>>(),
    ))
    .into()
}

pub fn row(p: &Parameter) -> EditorParam {
    let d = p.delay_buffer.clone().unwrap_or(DelaySettings {
        ref_param: String::new(),
        smoothing: 1.0,
        delay_count: 1,
        in_min: 0.0,
        in_max: 1.0,
        out_min: 0.0,
        out_max: 1.0,
    });
    EditorParam {
        name: p.name.clone().into(),
        func: p.func.clone().into(),
        min: p.min as f32,
        max: p.max as f32,
        default_value: p.default_value as f32,
        is_delay: p.delay_buffer.is_some(),
        ref_param: d.ref_param.into(),
        smoothing: d.smoothing as f32,
        delay_count: d.delay_count as f32,
        in_min: d.in_min as f32,
        in_max: d.in_max as f32,
        out_min: d.out_min as f32,
        out_max: d.out_max as f32,
    }
}

pub fn parameter(p: EditorParam) -> Parameter {
    Parameter {
        name: p.name.to_string(),
        func: p.func.to_string(),
        min: p.min as f64,
        max: p.max as f64,
        default_value: p.default_value as f64,
        delay_buffer: p.is_delay.then(|| DelaySettings {
            ref_param: p.ref_param.to_string(),
            smoothing: p.smoothing as f64,
            delay_count: p.delay_count as usize,
            in_min: p.in_min as f64,
            in_max: p.in_max as f64,
            out_min: p.out_min as f64,
            out_max: p.out_max as f64,
        }),
    }
}

pub fn status_text(status: NetworkStatus) -> String {
    match status {
        NetworkStatus::Stopped => "Stopped".into(),
        NetworkStatus::Connecting => "Connecting".into(),
        NetworkStatus::Authorizing => "Authorize in VTube Studio".into(),
        NetworkStatus::Connected => "Connected".into(),
        NetworkStatus::Error(e) => e,
    }
}

pub(crate) fn face_present_after_timeout(
    frame_present: bool,
    last_frame: Option<Instant>,
    timeout: Duration,
) -> bool {
    frame_present && last_frame.is_some_and(|time| time.elapsed() < timeout)
}
