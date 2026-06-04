use std::cell::RefCell;
use std::rc::Rc;

use slint::Model;
use snenk_bridge_service::{preset::SnekPreset, vitamins};

use crate::presets::build_snek_preset;
use crate::preview::{
    collect_input_values, eval_outputs, rebuild_preview_state, update_outputs_and_mesh,
};
use crate::{App, EditorParam, NameValuePair};

/// Convert a stored `CalcFn` into the editor's row representation, expanding
/// any delay buffer into the row's delay fields.
pub fn calc_fn_to_editor_param(p: &vitamins::CalcFn) -> EditorParam {
    let db = p.delay_buffer.as_ref();
    EditorParam {
        name: p.name.clone().into(),
        func: p.func.clone().into(),
        min: p.min as f32,
        max: p.max as f32,
        default_value: p.default_value as f32,
        is_delay: db.is_some(),
        ref_param: db.map(|d| d.ref_param.clone()).unwrap_or_default().into(),
        smoothing: db.map_or(0.0, |d| d.smoothing as f32),
        delay_count: db.map_or(0.0, |d| d.delay_count as f32),
        in_min: db.map_or(0.0, |d| d.in_min as f32),
        in_max: db.map_or(0.0, |d| d.in_max as f32),
        out_min: db.map_or(0.0, |d| d.out_min as f32),
        out_max: db.map_or(0.0, |d| d.out_max as f32),
    }
}

/// Convert an editor row back into a `CalcFn`, rebuilding the delay buffer when
/// the row is a delay-buffer parameter.
pub fn editor_param_to_calc_fn(p: &EditorParam) -> vitamins::CalcFn {
    let delay_buffer = p.is_delay.then(|| vitamins::DelayBuffer {
        ref_param: p.ref_param.to_string(),
        smoothing: p.smoothing as f64,
        delay_count: p.delay_count.max(0.0) as usize,
        in_min: p.in_min as f64,
        in_max: p.in_max as f64,
        out_min: p.out_min as f64,
        out_max: p.out_max as f64,
    });
    vitamins::CalcFn {
        name: p.name.to_string(),
        func: p.func.to_string(),
        min: p.min as f64,
        max: p.max as f64,
        default_value: p.default_value as f64,
        delay_buffer,
    }
}

/// Load a preset's params into the editor fields and reset the dirty flag.
pub fn load_preset_into_editor(
    ui: &App,
    preset_name: &str,
    editor_params_model: &Rc<slint::VecModel<EditorParam>>,
    loaded_preset: &Rc<RefCell<String>>,
) {
    let snek = match build_snek_preset(preset_name) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("load_preset_into_editor: {e}");
            return;
        }
    };
    // Record which preset is actually loaded, independent of the editable
    // title field, so the switch guard can detect real preset changes.
    *loaded_preset.borrow_mut() = preset_name.to_string();
    ui.set_editor_title(snek.title.into());
    ui.set_editor_author(snek.author.into());
    ui.set_editor_description(snek.description.into());

    // Replace contents of the shared model so existing binding stays valid.
    while editor_params_model.row_count() > 0 {
        editor_params_model.remove(editor_params_model.row_count() - 1);
    }
    for p in &snek.params {
        editor_params_model.push(calc_fn_to_editor_param(p));
    }

    ui.set_has_unsaved_changes(false);
}

/// Full preview refresh after an editor edit: recompile, re-evaluate the output
/// panel, and redraw the mesh (delay buffers are not stepped here).
pub fn refresh_editor_preview(
    ui: &App,
    model: &slint::VecModel<EditorParam>,
    input_model: &slint::VecModel<NameValuePair>,
) {
    rebuild_preview_state(ui, model);
    let values = collect_input_values(input_model);
    let output = eval_outputs(&values, false);
    update_outputs_and_mesh(ui, &output);
}

/// Build a SnekPreset from the current editor state in the UI.
pub fn build_snek_from_editor(ui: &App) -> SnekPreset {
    finish_snek(ui, collect_editor_params(&ui.get_editor_params()))
}

/// Build a SnekPreset from the shared editor params VecModel (avoids double-borrow).
pub fn build_snek_from_editor_model(
    ui: &App,
    editor_params_model: &slint::VecModel<EditorParam>,
) -> SnekPreset {
    finish_snek(ui, collect_editor_params(editor_params_model))
}

/// Collect all params (expressions and delay buffers) from an editor model.
fn collect_editor_params(model: &impl slint::Model<Data = EditorParam>) -> Vec<vitamins::CalcFn> {
    (0..model.row_count())
        .filter_map(|i| model.row_data(i).map(|p| editor_param_to_calc_fn(&p)))
        .collect()
}

/// Apply editor metadata (title/author/description) around a param list.
fn finish_snek(ui: &App, params: Vec<vitamins::CalcFn>) -> SnekPreset {
    let mut snek = SnekPreset::new(ui.get_editor_title().to_string(), params);
    snek.author = ui.get_editor_author().to_string();
    snek.description = ui.get_editor_description().to_string();
    snek
}
