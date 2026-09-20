use crate::model::TrackingFrame;
use crate::network::MAX_FRAME;

use super::vts::{canonical, vector};

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
                        .ok_or_else(|| "Invalid pose number".to_owned())
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
pub struct Frames {
    buffer: Vec<u8>,
    discarding: bool,
}

impl Frames {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
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
