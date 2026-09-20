use std::collections::BTreeMap;

pub fn mapping() -> BTreeMap<String, String> {
    let mut names = BTreeMap::new();
    let roots = [
        ("jawOpen", "JawOpen"),
        ("jawForward", "JawForward"),
        ("jawLeft", "JawLeft"),
        ("jawRight", "JawRight"),
        ("mouthClose", "MouthClose"),
        ("mouthFunnel", "MouthFunnel"),
        ("mouthPucker", "MouthPucker"),
        ("mouthLeft", "MouthLeft"),
        ("mouthRight", "MouthRight"),
        ("mouthSmile", "MouthSmile"),
        ("mouthFrown", "MouthFrown"),
        ("mouthDimple", "MouthDimple"),
        ("mouthStretch", "MouthStretch"),
        ("mouthRollLower", "MouthRollLower"),
        ("mouthRollUpper", "MouthRollUpper"),
        ("mouthShrugLower", "MouthShrugLower"),
        ("mouthShrugUpper", "MouthShrugUpper"),
        ("mouthPress", "MouthPress"),
        ("mouthLowerDown", "MouthLowerDown"),
        ("mouthUpperUp", "MouthUpperUp"),
        ("cheekPuff", "CheekPuff"),
        ("cheekSquint", "CheekSquint"),
        ("noseSneer", "NoseSneer"),
        ("tongueOut", "TongueOut"),
        ("headPosX", "HeadPosX"),
        ("headPosY", "HeadPosY"),
        ("headPosZ", "HeadPosZ"),
        ("headRotX", "HeadRotX"),
        ("headRotY", "HeadRotY"),
        ("headRotZ", "HeadRotZ"),
        ("eyeLookDown", "EyeLookDown"),
        ("eyeLookIn", "EyeLookIn"),
        ("eyeLookOut", "EyeLookOut"),
        ("eyeLookUp", "EyeLookUp"),
        ("eyeBlink", "EyeBlink"),
        ("eyeSquint", "EyeSquint"),
        ("eyeWide", "EyeWide"),
        ("browDown", "BrowDown"),
        ("browInnerUp", "BrowInnerUp"),
        ("browOuterUp", "BrowOuterUp"),
    ];
    for (source, canonical) in roots {
        names.insert(source.into(), canonical.into());
        for (suffix, side) in [
            ("_L", "Left"),
            ("_l", "Left"),
            ("_left", "Left"),
            ("_Left", "Left"),
            ("_R", "Right"),
            ("_r", "Right"),
            ("_right", "Right"),
            ("_Right", "Right"),
        ] {
            names.insert(format!("{source}{suffix}"), format!("{canonical}{side}"));
        }
    }
    names
}

pub fn convert_expression(source: &str) -> String {
    let source = source
        .trim()
        .strip_prefix("return ")
        .unwrap_or(source.trim());
    let source = source
        .split("//")
        .next()
        .unwrap_or("")
        .trim()
        .trim_end_matches(';')
        .trim();
    let names = mapping();
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_alphabetic() || chars[i] == '_' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1
            }
            let name: String = chars[start..i].iter().collect();
            let eligible = start == 0
                || !(chars[start - 1].is_alphanumeric()
                    || chars[start - 1] == '_'
                    || chars[start - 1] == '.');
            if eligible && name == "Math" && i < chars.len() && chars[i] == '.' {
                let mut end = i + 1;
                while end < chars.len() && chars[end].is_alphanumeric() {
                    end += 1
                }
                let member: String = chars[i + 1..end].iter().collect();
                if member == "PI" {
                    out.push_str("math::pi()");
                    i = end;
                    continue;
                }
                if [
                    "abs", "min", "max", "sin", "cos", "floor", "ceil", "sqrt", "pow",
                ]
                .contains(&member.as_str())
                {
                    out.push_str("math::");
                    out.push_str(&member);
                    i = end;
                    continue;
                }
            }
            out.push_str(if eligible {
                names.get(&name).map(String::as_str).unwrap_or(&name)
            } else {
                &name
            });
        } else {
            out.push(chars[i]);
            i += 1
        }
    }
    out
}
