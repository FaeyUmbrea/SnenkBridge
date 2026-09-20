use std::{cell::RefCell, rc::Rc};

use slint::{ComponentHandle, SharedString, VecModel};

use crate::evaluation::{self, Evaluator};
use crate::presets;
use crate::settings;
use crate::{App, ImportDialog, NewPresetDialog, UnsavedChangesDialog, VariablesWindow};

use super::{
    convert::{row, strings},
    preview,
    types::{Action, Choice, State},
};

pub struct Ui {
    pub app: slint::Weak<App>,
    pub state: RefCell<State>,
    pub new: NewPresetDialog,
    pub import: ImportDialog,
    pub guard: UnsavedChangesDialog,
    pub variables: VariablesWindow,
}

impl Ui {
    pub fn error(&self, message: impl Into<SharedString>) {
        if let Some(app) = self.app.upgrade() {
            app.set_error_text(message.into());
        }
    }

    pub fn persist(&self) {
        if let Some(a) = self.app.upgrade() {
            settings::save_settings(&settings::Settings {
                preset_name: self.state.borrow().preset.title.clone(),
                phone_ip: a.get_phone_ip().to_string(),
                tracking_type_index: a.get_tracking_type_index(),
                face_search_timeout: a.get_face_search_timeout().to_string(),
                vts_ip: a.get_vts_ip().to_string(),
                vts_port: a.get_vts_port().to_string(),
            });
        }
    }

    pub fn sync(&self) {
        let Some(a) = self.app.upgrade() else { return };
        let s = self.state.borrow();
        a.set_preset_names(strings(s.choices.iter().map(|c| c.preset.title.clone())));
        a.set_preset_index(s.selected as i32);
        a.set_can_delete_preset(s.stored_selection);
        a.set_editor_title(s.preset.title.clone().into());
        a.set_editor_author(s.preset.author.clone().into());
        a.set_editor_description(s.preset.description.clone().into());
        a.set_editor_params(
            Rc::new(VecModel::from(
                s.preset.params.iter().map(row).collect::<Vec<_>>(),
            ))
            .into(),
        );
        a.set_editor_param_errors(strings(evaluation::validation_errors(&s.preset.params)));
    }

    pub fn rebuild(&self) {
        let mut s = self.state.borrow_mut();
        match Evaluator::new(&s.preset.params) {
            Ok(e) => {
                s.evaluator = Some(e);
                self.error("");
            }
            Err(e) => {
                s.evaluator = None;
                self.error(e);
            }
        }
        if let Some(a) = self.app.upgrade() {
            a.set_editor_param_errors(strings(evaluation::validation_errors(&s.preset.params)));
        }
        drop(s);
        self.preview(false);
    }

    pub fn dirty(&self) {
        if let Some(a) = self.app.upgrade() {
            a.set_has_unsaved_changes(true);
        }
        self.state.borrow_mut().preview_dirty = true;
        self.rebuild();
    }

    pub fn save(&self) -> bool {
        let mut s = self.state.borrow_mut();
        if s.preset.title.trim().is_empty() {
            self.error("A preset needs a title.");
            return false;
        }
        match s.store.save(&s.preset) {
            Ok(filename) => {
                let choice = Choice {
                    preset: s.preset.clone(),
                    filename: Some(filename.clone()),
                };
                if !s.stored_selection && s.selected < s.choices.len() {
                    let selected = s.selected;
                    s.choices[selected] = choice;
                } else if let Some(i) = s
                    .choices
                    .iter()
                    .position(|c| c.filename.as_ref() == Some(&filename))
                {
                    s.choices[i] = choice;
                    s.selected = i;
                } else {
                    s.choices.push(choice);
                    s.selected = s.choices.len() - 1;
                }
                s.stored_selection = true;
                drop(s);
                if let Some(a) = self.app.upgrade() {
                    a.set_has_unsaved_changes(false);
                }
                self.sync();
                self.persist();
                self.error("");
                true
            }
            Err(e) => {
                self.error(e);
                false
            }
        }
    }

    pub fn request(&self, action: Action) {
        let Some(a) = self.app.upgrade() else { return };
        if a.get_has_unsaved_changes() {
            self.state.borrow_mut().pending = Some(action);
            a.set_preset_index(self.state.borrow().selected as i32);
            if let Err(e) = self.guard.show() {
                self.error(e.to_string());
            }
        } else {
            self.execute(action);
        }
    }

    pub fn execute(&self, action: Action) {
        match action {
            Action::Switch(index) => {
                let mut s = self.state.borrow_mut();
                if let Some(c) = s.choices.get(index).cloned() {
                    s.selected = index;
                    s.preset = c.preset;
                    s.stored_selection = c.filename.is_some();
                }
                drop(s);
                if let Some(a) = self.app.upgrade() {
                    a.set_has_unsaved_changes(false);
                }
                self.sync();
                self.rebuild();
                self.persist();
            }
            Action::New => {
                self.new.set_preset_title("New preset".into());
                self.new.set_preset_author("".into());
                self.new.set_preset_description("".into());
                self.new.set_base_preset_names(strings(
                    std::iter::once("Empty".into()).chain(
                        self.state
                            .borrow()
                            .choices
                            .iter()
                            .map(|c| c.preset.title.clone()),
                    ),
                ));
                self.new.set_base_preset_index(0);
                let _ = self.new.show();
            }
            Action::Import => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Preset", &["snek", "json", "vps"])
                    .pick_file()
                {
                    match std::fs::read_to_string(&path) {
                        Ok(text) => {
                            let vitamins = path
                                .extension()
                                .is_some_and(|e| e.eq_ignore_ascii_case("vps"));
                            let parsed = if vitamins {
                                presets::import_vitamins(&text, false).map(|r| r.preset)
                            } else {
                                presets::parse(&text)
                            };
                            match parsed {
                                Ok(p) => {
                                    self.import.set_preset_title(p.title.into());
                                    self.import.set_preset_author(p.author.into());
                                    self.import.set_preset_description(p.description.into());
                                    self.import.set_show_swap_toggle(vitamins);
                                    self.import.set_swap_xy(false);
                                    self.state.borrow_mut().import_text = Some((text, vitamins));
                                    let _ = self.import.show();
                                }
                                Err(e) => self.error(e),
                            }
                        }
                        Err(e) => self.error(format!("Could not read preset: {e}")),
                    }
                }
            }
            Action::Delete => {
                let mut s = self.state.borrow_mut();
                if !s.stored_selection {
                    return;
                }
                if let Some(filename) = s.choices.get(s.selected).and_then(|c| c.filename.clone()) {
                    match s.store.delete(&filename) {
                        Ok(()) => {
                            let index = s.selected;
                            s.choices.remove(index);
                            s.selected = 0;
                            s.preset = s.choices[0].preset.clone();
                            s.stored_selection = s.choices[0].filename.is_some();
                        }
                        Err(e) => {
                            self.error(e);
                            return;
                        }
                    }
                }
                drop(s);
                if let Some(a) = self.app.upgrade() {
                    a.set_has_unsaved_changes(false);
                }
                self.sync();
                self.rebuild();
                self.persist();
            }
            Action::Close => {
                self.persist();
                let mut s = self.state.borrow_mut();
                s.source = None;
                s.target = None;
                drop(s);
                let _ = slint::quit_event_loop();
            }
        }
    }

    pub fn finish_guard(&self, save: bool) {
        if save && !self.save() {
            return;
        }
        let _ = self.guard.hide();
        let action = self.state.borrow_mut().pending.take();
        if let Some(action) = action {
            if let Some(a) = self.app.upgrade() {
                a.set_has_unsaved_changes(false);
            }
            self.execute(action);
        }
    }

    pub fn preview(&self, advance: bool) {
        preview::preview(self, advance);
    }

    pub fn tick(&self) {
        preview::tick(self);
    }
}
