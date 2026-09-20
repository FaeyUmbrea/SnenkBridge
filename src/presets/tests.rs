use super::*;

#[test]
fn imports_comma_declarations_and_reference_alias() {
    let text = r#"{"saveName":"Axes","customParam":[
        {"sendFlag":"true","paramName":"param_FaceAngleX","func":"let p=ref.FaceAngleX; let outmin=-30, outmax=30;","min":0,"max":1,"default":0}
    ]}"#;
    let result = import_vitamins(text, false).unwrap();
    let parameter = &result.preset.params[0];
    let delay = parameter.delay_buffer.as_ref().unwrap();
    assert_eq!(delay.ref_param, "FaceAngleX");
    assert_eq!(delay.out_min, -30.0);
    assert_eq!(delay.out_max, 30.0);
}
