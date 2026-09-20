use serde_json::Value;

use crate::model::TrackingFrame;
use crate::network::MAX_FRAME;

pub fn number(v: &Value) -> Result<f64, String> {
    v.as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| "Invalid numeric tracking value".into())
}

pub fn canonical(name: &str) -> Option<String> {
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

pub fn vector(frame: &mut TrackingFrame, prefix: &str, values: &[f64]) {
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
