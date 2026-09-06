use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use slint::Model;

use crate::editor::editor_param_to_calc_fn;
use crate::face_mesh;
use crate::{App, EditorParam, NameValuePair};

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

    /// Persistent model backing the output-params panel, updated in place
    /// so the value meters can refresh at the render frame rate.
    static OUTPUT_MODEL: Rc<slint::VecModel<NameValuePair>> = Rc::new(slint::VecModel::default());
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
                .map(|pair| (pair.name.to_string(), pair.value as f64))
        })
        .collect()
}

/// Evaluate current preview outputs given a set of input values.
///
/// If `advance_delays` is true, delay buffers advance one step (called from
/// the live tracking loop); if false, they evaluate in read-only / peek mode
/// (called from the editor preview when a slider moves).
pub fn eval_outputs(
    input_values: &HashMap<String, f64>,
    advance_delays: bool,
) -> Vec<NameValuePair> {
    PREVIEW_STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        let eval_results = snenk_bridge_service::eval::evaluate(&state.compiled, input_values);
        let mut map: HashMap<String, f64> = eval_results
            .into_iter()
            .map(|r| (r.name, r.value))
            .collect();

        // Evaluate delay buffers against the current values map (including
        // the newly-computed expression results).
        for (name, delay_state) in &mut state.delays {
            let src_name = delay_state.ref_param();
            let input_val = map
                .get(src_name)
                .copied()
                .or_else(|| input_values.get(src_name).copied())
                .unwrap_or(0.0);
            let out_val = if advance_delays {
                delay_state.update(input_val)
            } else {
                delay_state.current()
            };
            map.insert(name.clone(), out_val);
        }

        let mut output = Vec::new();
        for (name, &(min, max)) in &state.ranges {
            let val = map.get(name).copied().unwrap_or(0.0) as f32;
            output.push(NameValuePair {
                name: name.clone().into(),
                value: val,
                min,
                max,
            });
        }
        output
    })
}

/// Project and render the 3D GLB mesh for the given input values and update
/// the rendered model image in the UI.
pub fn update_input_mesh(ui: &App, input_values: &HashMap<String, f64>) {
    let image = face_mesh::compute_input_preview(|target_name| {
        input_values
            .get(target_name)
            .copied()
            .map(|val| val as f32)
            .or_else(|| {
                input_values
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(target_name))
                    .map(|(_, &val)| val as f32)
            })
    });
    ui.set_mesh_image(image);
}

/// Push an evaluated output list to the side panel (in place) and refresh the
/// input shape face mesh from the input values.
pub fn update_outputs_and_mesh(
    ui: &App,
    input_values: &HashMap<String, f64>,
    output: &[NameValuePair],
) {
    update_input_mesh(ui, input_values);
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
