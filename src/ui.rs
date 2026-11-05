use std::sync::Arc;
use std::time::Instant;

use eframe::egui;
use zeroize::Zeroize;

use super::{App, Form, LockState, Notice, OpenState, Phase, UnlockStep, CLIPBOARD_TTL};
use crate::clipboard;
use crate::vault::{self, Entry, Vault};

const BG: egui::Color32 = egui::Color32::from_rgb(16, 16, 18);
const CARD: egui::Color32 = egui::Color32::from_rgb(26, 26, 30);
const LINE: egui::Color32 = egui::Color32::from_rgb(46, 46, 52);
const TEXT: egui::Color32 = egui::Color32::from_rgb(236, 236, 238);
const MUTED: egui::Color32 = egui::Color32::from_rgb(138, 138, 146);
const DANGER: egui::Color32 = egui::Color32::from_rgb(214, 120, 114);
const FIELD_H: f32 = 44.0;
const BUTTON_H: f32 = 42.0;
const COPY_BTN: f32 = 36.0;
const LOGO_PX: usize = 256;

enum Action {
    None,
    Save,
    Lock,
}

enum FormAction {
    None,
    Save,
    Cancel,
}

enum CardAction {
    CopyLogin,
    CopyPassword,
    ToggleReveal,
    Edit,
    Delete,
    OpenSite,
}

pub(crate) fn install(ctx: &egui::Context) {
    install_fonts(ctx);

    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = BG;
    visuals.panel_fill = BG;
    visuals.extreme_bg_color = egui::Color32::from_rgb(12, 12, 14);
    visuals.faint_bg_color = CARD;
    visuals.code_bg_color = CARD;
    visuals.hyperlink_color = TEXT;
    visuals.error_fg_color = DANGER;
    visuals.window_stroke = egui::Stroke::NONE;
    visuals.selection.bg_fill = egui::Color32::from_rgb(58, 58, 66);

    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::same(8);
    }
    visuals.widgets.noninteractive.fg_stroke.color = TEXT;
    visuals.widgets.inactive.fg_stroke.color = TEXT;
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(36, 36, 40);
    visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(36, 36, 40);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(48, 48, 54);
    visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(48, 48, 54);
    visuals.widgets.hovered.fg_stroke.color = egui::Color32::WHITE;
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(58, 58, 64);
    visuals.widgets.active.weak_bg_fill = egui::Color32::from_rgb(58, 58, 64);
    visuals.widgets.active.fg_stroke.color = egui::Color32::WHITE;
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    style.spacing.button_padding = egui::vec2(16.0, 10.0);
    style.spacing.interact_size.y = BUTTON_H;
    ctx.set_style(style);
}

fn command_without_alt(input: &egui::InputState, key: egui::Key) -> bool {
    input.modifiers.command && !input.modifiers.alt && input.key_pressed(key)
}

pub(crate) fn show(app: &mut App, ctx: &egui::Context) {
    if ctx.input(|input| command_without_alt(input, egui::Key::L))
        && matches!(app.phase, Phase::Open(_))
    {
        if let Err(message) = app.lock() {
            if let Phase::Open(open) = &mut app.phase {
                open.notice = Some(Notice::Error(message));
            }
        }
    }

    let path = app.path.clone();
    let action = match &mut app.phase {
        Phase::Locked(lock) => {
            lock_screen(lock, &path, ctx);
            Action::None
        }
        Phase::Open(open) => open_screen(open, ctx),
    };

    match action {
        Action::None => {}
        Action::Save => match app.save() {
            Ok(()) => {
                if let Phase::Open(open) = &mut app.phase {
                    open.notice = Some(Notice::Saved);
                }
            }
            Err(message) => {
                if let Phase::Open(open) = &mut app.phase {
                    open.notice = Some(Notice::Error(message));
                }
            }
        },
        Action::Lock => {
            if let Err(message) = app.lock() {
                if let Phase::Open(open) = &mut app.phase {
                    open.notice = Some(Notice::Error(message));
                }
            }
        }
    }
}

fn lock_screen(lock: &mut LockState, path: &std::path::Path, ctx: &egui::Context) {
    let mut submit = false;
    egui::CentralPanel::default()
        .frame(panel())
        .show(ctx, |ui| {
            let top = (ui.available_height() * 0.14).clamp(28.0, 110.0);
            ui.add_space(top);

            let column = ui.available_width().min(320.0);
            let side = ((ui.available_width() - column) * 0.5).max(0.0);
            ui.horizontal(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    ui.set_width(column);
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("passkeep").size(30.0).color(TEXT));
                    });
                    ui.add_space(6.0);
                    let subtitle = if lock.creating {
                        "New vault. The master password needs at least 8 characters."
                    } else {
                        "Enter the master password to unlock the vault."
                    };
                    ui.label(egui::RichText::new(subtitle).size(13.0).color(MUTED));
                    ui.add_space(22.0);

                    let busy = lock.step != UnlockStep::Idle;
                    ui.add_enabled_ui(!busy, |ui| {
                        let password = ui.add(text_field(
                            &mut lock.password,
                            "master password",
                            !lock.show_password,
                        ));
                        if lock.autofocus && !busy {
                            password.request_focus();
                            lock.autofocus = false;
                        }

                        let mut confirm_enter = false;
                        if lock.creating {
                            ui.add_space(8.0);
                            let confirm = ui.add(text_field(
                                &mut lock.confirm,
                                "repeat password",
                                !lock.show_password,
                            ));
                            confirm_enter = confirm.lost_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Enter));
                            if password.lost_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Enter))
                            {
                                confirm.request_focus();
                            }
                        } else if password.lost_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter))
                        {
                            submit = true;
                        }
                        if confirm_enter {
                            submit = true;
                        }

                        ui.add_space(6.0);
                        ui.checkbox(&mut lock.show_password, "show password");
                        ui.add_space(12.0);

                        let label = if busy {
                            "checking…"
                        } else if lock.creating {
                            "create vault"
                        } else {
                            "unlock"
                        };
                        if wide_button(ui, label).clicked() {
                            submit = true;
                        }
                    });

                    if let Some(error) = &lock.error {
                        ui.add_space(10.0);
                        ui.label(egui::RichText::new(error).color(DANGER).size(13.0));
                    }

                    ui.add_space(28.0);
                    ui.label(
                        egui::RichText::new(path.display().to_string())
                            .size(11.0)
                            .color(MUTED),
                    );
                });
            });
        });

    if submit && lock.step == UnlockStep::Idle {
        if let Some(message) = validate_master(lock) {
            lock.error = Some(message);
        } else {
            lock.error = None;
            lock.step = UnlockStep::ShowBusy;
            ctx.request_repaint();
        }
    }
}

fn validate_master(lock: &LockState) -> Option<String> {
    let length = lock.password.chars().count();
    if length < 8 {
        return Some("master password needs at least 8 characters".to_owned());
    }
    if length > 1024 {
        return Some("master password is too long".to_owned());
    }
    if lock.creating && lock.password != lock.confirm {
        return Some("passwords do not match".to_owned());
    }
    None
}

fn open_screen(open: &mut OpenState, ctx: &egui::Context) -> Action {
    let mut lock_clicked = false;
    let mut save_needed = false;
    let mut start_new = false;
    let mut form_action = FormAction::None;
    let mut card: Option<(String, CardAction)> = None;

    let (find_shortcut, new_shortcut, esc) = ctx.input(|input| {
        (
            command_without_alt(input, egui::Key::F),
            command_without_alt(input, egui::Key::N),
            input.key_pressed(egui::Key::Escape),
        )
    });
    if find_shortcut {
        open.focus_search = true;
    }
    if new_shortcut && open.form.is_none() {
        open.confirm_delete = None;
        open.form = Some(Form::blank());
    }
    if esc {
        if open.form.is_some() {
            open.form = None;
        } else if open.confirm_delete.is_some() {
            open.confirm_delete = None;
        } else {
            open.query.clear();
        }
    }

    let ids = visible_ids(&open.vault, &open.query);
    let total = open.vault.entries.len();

    egui::CentralPanel::default()
        .frame(panel())
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("passkeep").size(22.0).color(TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if action_button(ui, "lock").clicked() {
                        lock_clicked = true;
                    }
                });
            });
            ui.add_space(8.0);

            let search = name_search(ui, &mut open.query);
            if open.focus_search {
                search.request_focus();
                open.focus_search = false;
            }
            ui.add_space(8.0);

            if let Some(form) = open.form.as_mut() {
                form_action = form_editor(ui, form);
                ui.add_space(8.0);
            } else if wide_button(ui, "new entry").clicked() {
                start_new = true;
            }

            ui.add_space(4.0);
            let footer = if open.notice.is_some() { 56.0 } else { 28.0 };
            let scroll_height = (ui.available_height() - footer).max(80.0);
            egui::ScrollArea::vertical()
                .id_salt("entries")
                .max_height(scroll_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if ids.is_empty() {
                        ui.add_space(28.0);
                        let message = if total == 0 {
                            "nothing here yet"
                        } else {
                            "no matches"
                        };
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new(message).color(MUTED));
                        });
                        return;
                    }
                    for id in &ids {
                        let Some(entry) = open.vault.entries.iter().find(|entry| &entry.id == id)
                        else {
                            continue;
                        };
                        let revealed = open.revealed.contains(id);
                        let armed = open.confirm_delete.as_ref() == Some(id);
                        ui.push_id(id, |ui| {
                            if let Some(action) = entry_card(ui, entry, revealed, armed) {
                                card = Some((id.clone(), action));
                            }
                        });
                    }
                });

            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(count_label(total, ids.len(), open.query.trim().is_empty()))
                    .size(12.0)
                    .color(MUTED),
            );
            if let Some(notice) = &open.notice {
                ui.label(notice_text(notice).size(12.0));
            }
        });

    match form_action {
        FormAction::Cancel => open.form = None,
        FormAction::Save => {
            if let Some(mut form) = open.form.take() {
                if let Some(message) = validate_form(&form) {
                    form.error = Some(message);
                    open.form = Some(form);
                } else {
                    apply_form(&mut open.vault, &mut form);
                    open.dirty = true;
                    save_needed = true;
                    open.confirm_delete = None;
                }
            }
        }
        FormAction::None => {}
    }
    if start_new {
        open.confirm_delete = None;
        open.form = Some(Form::blank());
    }
    if let Some((id, action)) = card {
        match action {
            CardAction::CopyLogin => {
                if let Some(secret) = open
                    .vault
                    .entries
                    .iter()
                    .find(|entry| entry.id == id)
                    .map(|entry| entry.login.clone())
                {
                    remember_copy(open, "login", secret);
                }
            }
            CardAction::CopyPassword => {
                if let Some(secret) = open
                    .vault
                    .entries
                    .iter()
                    .find(|entry| entry.id == id)
                    .map(|entry| entry.password.clone())
                {
                    remember_copy(open, "password", secret);
                }
            }
            CardAction::ToggleReveal => {
                if !open.revealed.remove(&id) {
                    open.revealed.insert(id);
                }
            }
            CardAction::Edit => {
                if let Some(entry) = open.vault.entries.iter().find(|entry| entry.id == id) {
                    open.form = Some(Form {
                        id: Some(entry.id.clone()),
                        label: entry.label.clone(),
                        url: entry.url.clone(),
                        login: entry.login.clone(),
                        password: entry.password.clone(),
                        show_password: false,
                        error: None,
                        focus: true,
                    });
                }
                open.confirm_delete = None;
            }
            CardAction::Delete => {
                if open.confirm_delete.as_ref() == Some(&id) {
                    open.vault.entries.retain(|entry| entry.id != id);
                    open.revealed.remove(&id);
                    open.confirm_delete = None;
                    open.dirty = true;
                    save_needed = true;
                } else {
                    open.confirm_delete = Some(id);
                }
            }
            CardAction::OpenSite => {
                if let Some(url) = open
                    .vault
                    .entries
                    .iter()
                    .find(|entry| entry.id == id)
                    .map(|entry| entry.url.clone())
                {
                    if let Err(message) = open_website(&url) {
                        open.notice = Some(Notice::Error(message));
                    }
                }
            }
        }
    }

    if lock_clicked {
        Action::Lock
    } else if save_needed {
        Action::Save
    } else {
        Action::None
    }
}

fn form_editor(ui: &mut egui::Ui, form: &mut Form) -> FormAction {
    let mut action = FormAction::None;
    card_frame().show(ui, |ui| {
        let title = if form.id.is_some() {
            "edit entry"
        } else {
            "new entry"
        };
        ui.label(egui::RichText::new(title).strong().color(TEXT));
        ui.add_space(8.0);
        let label = labeled_edit(ui, "name", &mut form.label, false, false, "e.g. GitHub");
        labeled_edit(ui, "website", &mut form.url, false, false, "github.com");
        labeled_edit(ui, "login", &mut form.login, false, false, "username");
        labeled_edit(
            ui,
            "password",
            &mut form.password,
            !form.show_password,
            form.show_password,
            "password",
        );
        if form.focus {
            label.request_focus();
            form.focus = false;
        }
        ui.checkbox(&mut form.show_password, "show password");
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if action_button(ui, "save").clicked() {
                action = FormAction::Save;
            }
            if action_button(ui, "cancel").clicked() {
                action = FormAction::Cancel;
            }
        });
        if let Some(error) = &form.error {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(error).color(DANGER).size(13.0));
        }
    });
    ui.add_space(8.0);
    action
}

fn labeled_edit(
    ui: &mut egui::Ui,
    caption: &str,
    value: &mut String,
    mask: bool,
    monospace: bool,
    hint: &str,
) -> egui::Response {
    ui.label(egui::RichText::new(caption).size(12.0).color(MUTED));
    let mut edit = text_field(value, hint, mask);
    if monospace {
        edit = edit.font(egui::TextStyle::Monospace);
    }
    let response = ui.add(edit);
    ui.add_space(8.0);
    response
}

fn name_search(ui: &mut egui::Ui, value: &mut String) -> egui::Response {
    let response = ui.add(text_field_margin(value, "search by name", false, 38));
    let center = response.rect.left_center() + egui::vec2(20.0, 0.0);
    paint_search(ui.painter(), center, MUTED);
    response
}

fn text_field<'a>(value: &'a mut String, hint: &str, mask: bool) -> egui::TextEdit<'a> {
    text_field_margin(value, hint, mask, 14)
}

fn text_field_margin<'a>(
    value: &'a mut String,
    hint: &str,
    mask: bool,
    left: i8,
) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(value)
        .id_salt(hint)
        .hint_text(hint)
        .password(mask)
        .cursor_at_end(true)
        .desired_width(f32::INFINITY)
        .margin(egui::Margin {
            left,
            right: 14,
            top: 0,
            bottom: 0,
        })
        .vertical_align(egui::Align::Center)
        .min_size(egui::vec2(0.0, FIELD_H))
}

fn paint_search(painter: &egui::Painter, center: egui::Pos2, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.6, color);
    painter.circle_stroke(center + egui::vec2(-1.5, -1.5), 5.0, stroke);
    painter.line_segment(
        [center + egui::vec2(2.0, 2.0), center + egui::vec2(6.4, 6.4)],
        stroke,
    );
}

fn wide_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add_sized(
        [ui.available_width(), BUTTON_H],
        egui::Button::new(egui::RichText::new(label).size(15.0))
            .min_size(egui::vec2(0.0, BUTTON_H)),
    )
}

fn action_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).size(14.0)).min_size(egui::vec2(84.0, 36.0)),
    )
}

fn entry_card(ui: &mut egui::Ui, entry: &Entry, revealed: bool, armed: bool) -> Option<CardAction> {
    let mut action = None;
    card_frame().show(ui, |ui| {
        ui.label(
            egui::RichText::new(&entry.label)
                .size(16.0)
                .strong()
                .color(TEXT),
        );
        ui.add_space(6.0);
        if copy_row(ui, "login", &entry.login, false) {
            action = Some(CardAction::CopyLogin);
        }
        if copy_row(ui, "password", &entry.password, !revealed) {
            action = Some(CardAction::CopyPassword);
        }
        if site_row(ui, &entry.url) {
            action = Some(CardAction::OpenSite);
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let show_tip = if revealed {
                "Hide password"
            } else {
                "Show password"
            };
            if icon_button(ui, show_tip, false, |painter, center, color, _fill| {
                paint_eye(painter, center, color, revealed);
            })
            .clicked()
            {
                action = Some(CardAction::ToggleReveal);
            }
            if icon_button(ui, "Edit", false, paint_pencil).clicked() {
                action = Some(CardAction::Edit);
            }
            let delete_tip = if armed { "Delete?" } else { "Delete" };
            if icon_button(ui, delete_tip, armed, paint_trash).clicked() {
                action = Some(CardAction::Delete);
            }
        });
    });
    ui.add_space(8.0);
    action
}

fn copy_row(ui: &mut egui::Ui, name: &str, value: &str, mask: bool) -> bool {
    let mut copied = false;
    ui.horizontal(|ui| {
        ui.set_min_height(COPY_BTN);
        ui.add(egui::Label::new(
            egui::RichText::new(name).size(14.0).color(MUTED),
        ));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !value.is_empty()
                && icon_button(ui, &format!("Copy {name}"), false, paint_copy_icon).clicked()
            {
                copied = true;
            }
            let shown = if value.is_empty() {
                "—".to_owned()
            } else if mask {
                "••••••••".to_owned()
            } else {
                value.to_owned()
            };
            let mut text = egui::RichText::new(shown).size(14.0);
            if value.is_empty() || mask {
                text = text.color(MUTED);
            } else {
                text = text.monospace();
            }
            ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Truncate));
        });
    });
    copied
}

fn site_row(ui: &mut egui::Ui, url: &str) -> bool {
    if url.is_empty() {
        return false;
    }
    let mut open = false;
    ui.horizontal(|ui| {
        ui.set_min_height(COPY_BTN);
        ui.add(egui::Label::new(
            egui::RichText::new("site").size(14.0).color(MUTED),
        ));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icon_button(ui, "Open site", false, paint_open_site).clicked() {
                open = true;
            }
            ui.add(
                egui::Label::new(
                    egui::RichText::new(site_label(url))
                        .size(14.0)
                        .monospace()
                        .color(TEXT),
                )
                .truncate(),
            );
        });
    });
    open
}

fn site_label(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_owned()
}

fn icon_button(
    ui: &mut egui::Ui,
    tooltip: &str,
    armed: bool,
    paint: impl FnOnce(&egui::Painter, egui::Pos2, egui::Color32, egui::Color32),
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(COPY_BTN, COPY_BTN), egui::Sense::click());
    let hovered = response.hovered() || response.highlighted() || response.has_focus();
    let fill = if armed {
        egui::Color32::from_rgb(92, 36, 34)
    } else if hovered {
        egui::Color32::from_rgb(54, 54, 60)
    } else {
        egui::Color32::from_rgb(40, 40, 46)
    };
    let stroke = if armed {
        egui::Stroke::new(1.0, DANGER)
    } else if hovered {
        egui::Stroke::new(1.0, egui::Color32::from_rgb(110, 110, 120))
    } else {
        egui::Stroke::NONE
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(8),
        fill,
        stroke,
        egui::StrokeKind::Inside,
    );
    let color = if hovered && !armed {
        egui::Color32::WHITE
    } else {
        TEXT
    };
    paint(ui.painter(), rect.center(), color, fill);
    response.on_hover_text(tooltip)
}

fn paint_eye(painter: &egui::Painter, center: egui::Pos2, color: egui::Color32, slashed: bool) {
    let stroke = egui::Stroke::new(1.7, color);
    painter.add(egui::Shape::ellipse_stroke(
        center,
        egui::vec2(7.4, 4.6),
        stroke,
    ));
    painter.circle_filled(center, 1.8, color);
    if slashed {
        painter.line_segment(
            [
                center + egui::vec2(-8.2, 6.2),
                center + egui::vec2(8.2, -6.2),
            ],
            egui::Stroke::new(1.7, color),
        );
    }
}

fn paint_pencil(
    painter: &egui::Painter,
    center: egui::Pos2,
    color: egui::Color32,
    fill: egui::Color32,
) {
    let body = vec![
        center + egui::vec2(5.6, -7.4),
        center + egui::vec2(7.6, -5.4),
        center + egui::vec2(-4.4, 6.4),
        center + egui::vec2(-6.4, 4.4),
    ];
    let tip = vec![
        center + egui::vec2(-6.4, 4.4),
        center + egui::vec2(-4.4, 6.4),
        center + egui::vec2(-8.0, 8.0),
    ];
    painter.add(egui::Shape::convex_polygon(body, color, egui::Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(tip, color, egui::Stroke::NONE));
    painter.line_segment(
        [
            center + egui::vec2(2.4, -4.4),
            center + egui::vec2(4.6, -2.2),
        ],
        egui::Stroke::new(1.7, fill),
    );
}

fn paint_trash(
    painter: &egui::Painter,
    center: egui::Pos2,
    _color: egui::Color32,
    _fill: egui::Color32,
) {
    let stroke = egui::Stroke::new(1.7, DANGER);
    painter.line_segment(
        [
            center + egui::vec2(-6.4, -3.6),
            center + egui::vec2(6.4, -3.6),
        ],
        stroke,
    );
    painter.rect_stroke(
        egui::Rect::from_center_size(center + egui::vec2(0.0, -5.8), egui::vec2(4.4, 2.6)),
        egui::CornerRadius::same(1),
        stroke,
        egui::StrokeKind::Inside,
    );
    painter.rect_stroke(
        egui::Rect::from_min_size(center + egui::vec2(-5.2, -2.4), egui::vec2(10.4, 9.4)),
        egui::CornerRadius::same(2),
        stroke,
        egui::StrokeKind::Inside,
    );
    painter.line_segment(
        [
            center + egui::vec2(-2.2, -0.4),
            center + egui::vec2(-2.2, 4.8),
        ],
        stroke,
    );
    painter.line_segment(
        [
            center + egui::vec2(2.2, -0.4),
            center + egui::vec2(2.2, 4.8),
        ],
        stroke,
    );
}

fn paint_copy_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    color: egui::Color32,
    fill: egui::Color32,
) {
    let stroke = egui::Stroke::new(1.6, color);
    let size = egui::vec2(11.0, 11.0);
    let radius = egui::CornerRadius::same(2);
    let back = egui::Rect::from_min_size(center + egui::vec2(-7.0, -7.0), size);
    let front = egui::Rect::from_min_size(center + egui::vec2(-3.5, -3.5), size);
    painter.rect_stroke(back, radius, stroke, egui::StrokeKind::Inside);
    painter.rect(front, radius, fill, stroke, egui::StrokeKind::Inside);
}

fn paint_open_site(
    painter: &egui::Painter,
    center: egui::Pos2,
    color: egui::Color32,
    _fill: egui::Color32,
) {
    let stroke = egui::Stroke::new(1.6, color);
    let left = center.x - 6.0;
    let right = center.x + 2.5;
    let top = center.y - 1.5;
    let bottom = center.y + 7.0;
    painter.line_segment([egui::pos2(left, top), egui::pos2(left, bottom)], stroke);
    painter.line_segment(
        [egui::pos2(left, bottom), egui::pos2(right, bottom)],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(right, bottom), egui::pos2(right, top + 3.5)],
        stroke,
    );
    painter.line_segment([egui::pos2(left, top), egui::pos2(left + 4.0, top)], stroke);
    let tip = center + egui::vec2(6.5, -6.5);
    painter.line_segment([center + egui::vec2(0.5, -0.5), tip], stroke);
    painter.line_segment([tip, tip + egui::vec2(-4.6, 0.2)], stroke);
    painter.line_segment([tip, tip + egui::vec2(-0.2, 4.6)], stroke);
}

fn validate_form(form: &Form) -> Option<String> {
    if form.label.trim().is_empty() {
        return Some("enter what this entry is for".to_owned());
    }
    if form.label.trim().chars().count() > 200 {
        return Some("name is too long".to_owned());
    }
    if form.login.chars().count() > 500 {
        return Some("login is too long".to_owned());
    }
    if form.password.chars().count() > 2000 {
        return Some("password is too long".to_owned());
    }
    if let Err(message) = normalize_website(&form.url) {
        return Some(message);
    }
    None
}

fn normalize_website(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(String::new());
    }
    if raw.chars().count() > 2000 {
        return Err("website is too long".to_owned());
    }
    if raw.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("website link can't contain spaces".to_owned());
    }
    let (scheme, rest) = if let Some(rest) = strip_scheme(raw, "https://") {
        ("https", rest)
    } else if let Some(rest) = strip_scheme(raw, "http://") {
        ("http", rest)
    } else if raw.contains("://") {
        return Err("website must start with http:// or https://".to_owned());
    } else {
        ("https", raw)
    };
    if !website_host_ok(rest) {
        return Err("enter a website, for example github.com".to_owned());
    }
    Ok(format!("{scheme}://{rest}"))
}

fn strip_scheme<'a>(raw: &'a str, scheme: &str) -> Option<&'a str> {
    raw.get(..scheme.len())
        .filter(|prefix| prefix.eq_ignore_ascii_case(scheme))
        .map(|_| &raw[scheme.len()..])
}

fn website_host_ok(rest: &str) -> bool {
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.contains('@') || host.starts_with('[') {
        return false;
    }
    let name = host.split(':').next().unwrap_or("");
    if name.eq_ignore_ascii_case("localhost") {
        return true;
    }
    name.contains('.') && !name.starts_with('.') && !name.ends_with('.')
}

fn apply_form(vault: &mut Vault, form: &mut Form) {
    let label = form.label.trim().to_owned();
    let url = normalize_website(&form.url).unwrap_or_default();
    let login = std::mem::take(&mut form.login);
    let password = std::mem::take(&mut form.password);
    if let Some(id) = form.id.clone() {
        if let Some(entry) = vault.entries.iter_mut().find(|entry| entry.id == id) {
            entry.label = label;
            entry.url = url;
            entry.login = login;
            entry.password = password;
            return;
        }
    }
    vault.entries.push(Entry {
        id: vault::new_id(),
        label,
        url,
        login,
        password,
    });
}

fn remember_copy(open: &mut OpenState, what: &'static str, mut secret: String) {
    if secret.is_empty() {
        secret.zeroize();
        return;
    }
    match clipboard::copy(&secret) {
        Ok(()) => {
            open.copied = Some(super::Copied {
                secret: secret.clone(),
                until: Instant::now() + CLIPBOARD_TTL,
            });
            open.notice = Some(Notice::Copied(what));
        }
        Err(message) => open.notice = Some(Notice::Error(message)),
    }
    secret.zeroize();
}

fn visible_ids(vault: &Vault, query: &str) -> Vec<String> {
    let query = query.trim().to_lowercase();
    let mut entries: Vec<&Entry> = vault
        .entries
        .iter()
        .filter(|entry| query.is_empty() || entry.label.to_lowercase().contains(&query))
        .collect();
    entries.sort_by(|left, right| {
        let left_name = left.label.to_lowercase();
        let right_name = right.label.to_lowercase();
        right_name
            .starts_with(&query)
            .cmp(&left_name.starts_with(&query))
            .then_with(|| left_name.cmp(&right_name))
            .then_with(|| left.id.cmp(&right.id))
    });
    entries.into_iter().map(|entry| entry.id.clone()).collect()
}

fn count_label(total: usize, shown: usize, unfiltered: bool) -> String {
    if unfiltered {
        if total == 1 {
            "1 entry".to_owned()
        } else {
            format!("{total} entries")
        }
    } else {
        format!("{shown} of {total}")
    }
}

fn notice_text(notice: &Notice) -> egui::RichText {
    match notice {
        Notice::Saved => egui::RichText::new("saved").color(MUTED),
        Notice::Copied(what) => {
            egui::RichText::new(format!("{what} copied · clipboard clears in 30 s")).color(MUTED)
        }
        Notice::Error(message) => egui::RichText::new(message).color(DANGER),
    }
}

fn panel() -> egui::Frame {
    egui::Frame::new()
        .fill(BG)
        .inner_margin(egui::Margin::same(16))
}

fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .stroke(egui::Stroke::new(1.0, LINE))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(12))
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Some(segoe) = load_font(r"C:\Windows\Fonts\segoeui.ttf") {
        fonts.font_data.insert("segoe".to_owned(), segoe);
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "segoe".to_owned());
    }
    if let Some(consolas) = load_font(r"C:\Windows\Fonts\consola.ttf") {
        fonts.font_data.insert("consolas".to_owned(), consolas);
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "consolas".to_owned());
    }
    ctx.set_fonts(fonts);
}

fn load_font(path: &str) -> Option<Arc<egui::FontData>> {
    let bytes = std::fs::read(path).ok()?;
    Some(Arc::new(egui::FontData::from_owned(bytes)))
}

pub(crate) fn icon() -> egui::IconData {
    egui::IconData {
        rgba: include_bytes!("icon.rgba").to_vec(),
        width: LOGO_PX as u32,
        height: LOGO_PX as u32,
    }
}

fn open_website(url: &str) -> Result<(), String> {
    let url = normalize_website(url)?;
    if url.is_empty() {
        return Err("this entry has no website".to_owned());
    }
    #[cfg(windows)]
    {
        open_website_windows(&url)
    }
    #[cfg(not(windows))]
    {
        open_website_other(&url)
    }
}

#[cfg(windows)]
fn open_website_windows(url: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    fn wide(text: &str) -> Vec<u16> {
        std::ffi::OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let file = wide(url);
    let operation = wide("open");
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if (result as isize) <= 32 {
        Err("couldn't open the site".to_owned())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn open_website_other(url: &str) -> Result<(), String> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("couldn't open the site: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, label: &str, login: &str) -> Entry {
        Entry {
            id: id.to_owned(),
            label: label.to_owned(),
            url: String::new(),
            login: login.to_owned(),
            password: "secret".to_owned(),
        }
    }

    fn sample() -> Vault {
        Vault {
            entries: vec![
                entry("b", "Mail", "git@example.com"),
                entry("a", "GitHub", "octocat"),
                entry("c", "Bank", "me"),
            ],
        }
    }

    #[test]
    fn search_matches_the_name_only() {
        let ids = visible_ids(&sample(), "git");
        assert_eq!(ids, vec!["a".to_owned()]);
    }

    #[test]
    fn search_is_case_insensitive_and_partial() {
        let vault = sample();
        assert_eq!(visible_ids(&vault, "HUB"), vec!["a".to_owned()]);
        assert_eq!(visible_ids(&vault, "  mail "), vec!["b".to_owned()]);
    }

    #[test]
    fn names_that_start_with_the_query_come_first() {
        let vault = Vault {
            entries: vec![entry("1", "My GitHub", "a"), entry("2", "GitLab", "b")],
        };
        assert_eq!(
            visible_ids(&vault, "git"),
            vec!["2".to_owned(), "1".to_owned()]
        );
    }

    #[test]
    fn empty_query_lists_every_name_alphabetically() {
        assert_eq!(
            visible_ids(&sample(), "   "),
            vec!["c".to_owned(), "a".to_owned(), "b".to_owned()]
        );
    }

    #[test]
    fn website_without_a_scheme_becomes_https() {
        assert_eq!(
            normalize_website("github.com").unwrap(),
            "https://github.com"
        );
        assert_eq!(
            normalize_website("  HTTP://example.com/a  ").unwrap(),
            "http://example.com/a"
        );
        assert!(normalize_website("").unwrap().is_empty());
    }

    #[test]
    fn website_rejects_other_schemes() {
        assert!(normalize_website("javascript:alert(1)").is_err());
        assert!(normalize_website("file:///c:/secret").is_err());
        assert!(normalize_website("https://user:pass@github.com").is_err());
        assert!(normalize_website("not a url").is_err());
    }
}
