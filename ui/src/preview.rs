use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use slint::Model;

use crate::editor::editor_param_to_calc_fn;
use crate::face_mesh;
use crate::{App, EditorParam, MeshPoint, NameValuePair};

/// Cached, reusable preview evaluation state. Rebuilt only when the editor
/// params change; the live render loop then evaluates against it every frame
/// without re-parsing expressions.
struct PreviewState {
    compiled: Vec<snenk_bridge_service::eval::CompiledParam>,
    delays: Vec<(String, snenk_bridge_service::delay::DelayBufferState)>,
    ranges: HashMap<String, (f32, f32)>,
}

thread_local! {
    /// Compiled preview state. Thread-local because every access happens on the
    /// Slint event-loop thread.
    static PREVIEW_STATE: RefCell<PreviewState> = RefCell::new(PreviewState {
        compiled: Vec::new(),
        delays: Vec::new(),
        ranges: HashMap::new(),
    });

    /// Persistent model backing the face point cloud. Rows are updated in place
    /// each frame so Slint diffs them instead of recreating the repeater.
    static MESH_MODEL: Rc<slint::VecModel<MeshPoint>> = Rc::new(slint::VecModel::default());

    /// Persistent model backing the output-params panel, also updated in place
    /// so the value meters can refresh at the render frame rate.
    static OUTPUT_MODEL: Rc<slint::VecModel<NameValuePair>> = Rc::new(slint::VecModel::default());
}

/// The persistent point-cloud model, to bind onto the UI once at startup.
pub fn mesh_model() -> Rc<slint::VecModel<MeshPoint>> {
    MESH_MODEL.with(|m| m.clone())
}

/// The persistent output-params model, to bind onto the UI once at startup.
pub fn output_model() -> Rc<slint::VecModel<NameValuePair>> {
    OUTPUT_MODEL.with(|m| m.clone())
}

/// Update a value model in place: overwrite rows when the count matches,
/// otherwise rebuild. Avoids recreating the Slint repeater every frame.
fn update_value_model(model: &slint::VecModel<NameValuePair>, items: &[NameValuePair]) {
    if model.row_count() == items.len() {
        for (i, item) in items.iter().enumerate() {
            model.set_row_data(i, item.clone());
        }
    } else {
        while model.row_count() > 0 {
            model.remove(model.row_count() - 1);
        }
        for item in items {
            model.push(item.clone());
        }
    }
}

/// Recompile expressions, rebuild delay evaluators and ranges, and refresh the
/// per-row validation messages. Call whenever the editor params change.
pub fn rebuild_preview_state(ui: &App, model: &slint::VecModel<EditorParam>) {
    let calc_fns: Vec<snenk_bridge_service::vitamins::CalcFn> = (0..model.row_count())
        .filter_map(|i| model.row_data(i).map(|p| editor_param_to_calc_fn(&p)))
        .collect();

    let compiled = snenk_bridge_service::eval::compile_expressions(&calc_fns);
    let delays = calc_fns
        .iter()
        .filter_map(|f| {
            f.delay_buffer.clone().map(|db| {
                (
                    f.name.clone(),
                    snenk_bridge_service::delay::DelayBufferState::new(db),
                )
            })
        })
        .collect();
    let ranges = calc_fns
        .iter()
        .map(|f| (f.name.clone(), (f.min as f32, f.max as f32)))
        .collect();
    PREVIEW_STATE.with(|cell| {
        *cell.borrow_mut() = PreviewState {
            compiled,
            delays,
            ranges,
        };
    });

    // Validation only changes when the expressions change.
    let errors: Vec<slint::SharedString> = calc_fns
        .iter()
        .map(|f| {
            snenk_bridge_service::eval::validate_expression(&f.func)
                .map(slint::SharedString::from)
                .unwrap_or_default()
        })
        .collect();
    ui.set_editor_param_errors(Rc::new(slint::VecModel::from(errors)).into());
}

/// Snapshot the input-shape model into a name→value map for evaluation.
pub fn collect_input_values(input_model: &slint::VecModel<NameValuePair>) -> HashMap<String, f64> {
    (0..input_model.row_count())
        .filter_map(|i| {
            input_model
                .row_data(i)
                .map(|p| (p.name.to_string(), f64::from(p.value)))
        })
        .collect()
}

/// Evaluate the cached state into the full output list (with ranges) for the
/// side panel. `advance` steps the delay buffers.
pub fn eval_outputs(values: &HashMap<String, f64>, advance: bool) -> Vec<NameValuePair> {
    PREVIEW_STATE.with(|cell| {
        let st = &mut *cell.borrow_mut();
        let results = snenk_bridge_service::eval::evaluate(&st.compiled, values);
        let computed: HashMap<&str, f64> =
            results.iter().map(|r| (r.name.as_str(), r.value)).collect();

        let mut output: Vec<NameValuePair> = results
            .iter()
            .map(|r| {
                let (min, max) = st.ranges.get(r.name.as_str()).copied().unwrap_or((0.0, 1.0));
                NameValuePair {
                    name: r.name.clone().into(),
                    value: r.value as f32,
                    min,
                    max,
                }
            })
            .collect();

        for (name, ev) in st.delays.iter_mut() {
            let rv = computed.get(ev.ref_param()).copied();
            let v = match (advance, rv) {
                (true, Some(r)) => ev.update(r),
                _ => ev.current(),
            };
            let (min, max) = st.ranges.get(name.as_str()).copied().unwrap_or((0.0, 1.0));
            output.push(NameValuePair {
                name: name.clone().into(),
                value: v as f32,
                min,
                max,
            });
        }
        output
    })
}

/// Map well-known VTS output parameter names onto the normalised face-mesh
/// parameters. Head angles (degrees, ~-30..30) become rotation in radians.
pub fn face_params(get: impl Fn(&str) -> Option<f32>) -> face_mesh::FaceParams {
    let angle = |v: f32| (v / 30.0).clamp(-1.0, 1.0) * 0.6;
    let unit = |v: f32| v.clamp(-1.0, 1.0);
    face_mesh::FaceParams {
        yaw: get("FaceAngleX").map_or(0.0, angle),
        pitch: get("FaceAngleY").map_or(0.0, angle),
        roll: get("FaceAngleZ").map_or(0.0, angle),
        mouth_open: get("MouthOpen").unwrap_or(0.0).clamp(0.0, 1.0),
        mouth_smile: get("MouthSmile").map_or(0.0, unit),
        mouth_x: get("MouthX").map_or(0.0, unit),
        tongue_out: get("TongueOut").unwrap_or(0.0).clamp(0.0, 1.0),
        eye_open_l: get("EyeOpenLeft").unwrap_or(1.0).clamp(0.0, 1.0),
        eye_open_r: get("EyeOpenRight").unwrap_or(1.0).clamp(0.0, 1.0),
        brow_l: get("BrowLeftY").or_else(|| get("Brows")).map_or(0.0, unit),
        brow_r: get("BrowRightY").or_else(|| get("Brows")).map_or(0.0, unit),
    }
}

/// Project the mesh for the given params and update the persistent point model
/// (rows in place) plus the wireframe path.
pub fn update_mesh(ui: &App, params: &face_mesh::FaceParams) {
    let mesh = face_mesh::compute(params);
    MESH_MODEL.with(|model| {
        if model.row_count() == mesh.points.len() {
            for (i, &(x, y, depth)) in mesh.points.iter().enumerate() {
                model.set_row_data(i, MeshPoint { x, y, depth });
            }
        } else {
            while model.row_count() > 0 {
                model.remove(model.row_count() - 1);
            }
            for &(x, y, depth) in &mesh.points {
                model.push(MeshPoint { x, y, depth });
            }
        }
    });
    ui.set_mesh_wireframe(mesh.wireframe.into());
}

/// Push an evaluated output list to the side panel (in place) and refresh the
/// face mesh from the same values.
pub fn update_outputs_and_mesh(ui: &App, output: &[NameValuePair]) {
    let out_map: HashMap<&str, f32> = output.iter().map(|p| (p.name.as_str(), p.value)).collect();
    update_mesh(ui, &face_params(|n| out_map.get(n).copied()));
    OUTPUT_MODEL.with(|model| update_value_model(model, output));
}

/// Update the input-shapes panel from a name→value snapshot, in place.
pub fn update_input_model(
    input_model: &slint::VecModel<NameValuePair>,
    values: &HashMap<String, f64>,
) {
    for i in 0..input_model.row_count() {
        if let Some(mut pair) = input_model.row_data(i) {
            if let Some(&v) = values.get(pair.name.as_str()) {
                pair.value = v as f32;
                input_model.set_row_data(i, pair);
            }
        }
    }
}
