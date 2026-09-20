use std::{
    rc::Rc,
    time::{Duration, Instant},
};

use slint::VecModel;

use crate::evaluation;
use crate::face_mesh;
use crate::network::NetworkStatus;
use crate::NameValuePair;

use super::{
    actions::Ui,
    convert::{face_present_after_timeout, status_text},
    types::input_defaults,
};

pub fn preview(ui: &Ui, advance: bool) {
    let Some(a) = ui.app.upgrade() else { return };
    let mut s = ui.state.borrow_mut();
    let live = s.source.is_some();
    let mut values = if live {
        s.frame
            .as_ref()
            .map(|f| f.values.clone())
            .unwrap_or_else(input_defaults)
    } else {
        s.manual.clone()
    };
    let timeout = a.get_face_search_timeout().parse::<u64>().unwrap_or(3000);
    let face = if live {
        face_present_after_timeout(
            s.frame.as_ref().is_some_and(|f| f.face_present),
            s.last_frame,
            Duration::from_millis(timeout),
        )
    } else {
        values.get("FaceFound").copied().unwrap_or(0.0) > 0.0
    };
    values.insert("FaceFound".into(), f64::from(face));
    values.extend(evaluation::time_variables(
        &s.preset.params,
        s.session.elapsed().as_millis() as u64,
    ));
    let outputs = s
        .evaluator
        .as_mut()
        .map(|e| e.evaluate(&values, advance))
        .unwrap_or_default();
    s.last_outputs = Some(outputs.clone());
    s.last_preview_face = Some(face);
    if live && s.frame.is_some() {
        if let Some(target) = &s.target {
            target.publish(outputs.clone(), face);
        }
    }
    a.set_input_shapes(
        Rc::new(VecModel::from(
            values
                .iter()
                .map(|(name, value)| NameValuePair {
                    name: name.clone().into(),
                    value: *value as f32,
                    min: if name.starts_with("Head") {
                        -180.0
                    } else {
                        0.0
                    },
                    max: if name.starts_with("Head") { 180.0 } else { 1.0 },
                })
                .collect::<Vec<_>>(),
        ))
        .into(),
    );
    a.set_output_params(
        Rc::new(VecModel::from(
            s.preset
                .params
                .iter()
                .filter_map(|p| {
                    outputs.get(&p.name).map(|value| NameValuePair {
                        name: p.name.clone().into(),
                        value: *value as f32,
                        min: p.min as f32,
                        max: p.max as f32,
                    })
                })
                .collect::<Vec<_>>(),
        ))
        .into(),
    );
    a.set_mesh_image(face_mesh::compute_input_preview(|name| {
        values.get(name).map(|v| *v as f32)
    }));
    s.preview_dirty = false;
}

pub fn tick(ui: &Ui) {
    let Some(a) = ui.app.upgrade() else { return };
    let mut s = ui.state.borrow_mut();
    let mut frame_changed = false;
    if let Some(source) = &s.source {
        let (status, frame, count) = source.snapshot();
        if count != s.frame_count {
            s.frame_count = count;
            s.frame = frame;
            s.last_frame = Some(Instant::now());
            frame_changed = true;
        }
        let elapsed = s.rate_at.elapsed().as_secs_f64();
        if elapsed >= 1.0 {
            s.rate = count.saturating_sub(s.rate_count) as f64 / elapsed;
            s.rate_count = count;
            s.rate_at = Instant::now();
        }
        let text = match status {
            NetworkStatus::Connected => {
                if s.last_frame
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(1))
                {
                    format!("Receiving · {:.0} fps", s.rate)
                } else if s.frame.is_some() {
                    "Interrupted".into()
                } else {
                    "Awaiting data".into()
                }
            }
            other => status_text(other),
        };
        a.set_source_status(text.into());
        a.set_source_status_color(slint::Color::from_rgb_u8(
            if s.last_frame
                .is_some_and(|t| t.elapsed() < Duration::from_secs(1))
            {
                76
            } else {
                214
            },
            160,
            90,
        ));
    }
    if s.source.is_some() && s.frame.is_some() {
        let timeout = a.get_face_search_timeout().parse::<u64>().unwrap_or(3000);
        let face = face_present_after_timeout(
            s.frame.as_ref().is_some_and(|f| f.face_present),
            s.last_frame,
            Duration::from_millis(timeout),
        );
        if s.last_preview_face != Some(face) {
            s.preview_dirty = true;
        }
    }
    if let Some(target) = &s.target {
        let status = target.status();
        a.set_target_status_color(match status {
            NetworkStatus::Connected => slint::Color::from_rgb_u8(76, 191, 131),
            NetworkStatus::Error(_) => slint::Color::from_rgb_u8(196, 64, 57),
            _ => slint::Color::from_rgb_u8(214, 162, 60),
        });
        a.set_target_status(status_text(status).into());
    }
    let time_dependent = s
        .preset
        .params
        .iter()
        .any(|p| p.func.contains("Wave") || p.func.contains("PingPong"));
    let advance = s.source.is_some() && s.frame.is_some();
    let should_preview = s.preview_dirty || (advance && (frame_changed || time_dependent));
    if should_preview {
        s.preview_dirty = true;
    }
    drop(s);
    if should_preview {
        preview(ui, advance);
    }
}
