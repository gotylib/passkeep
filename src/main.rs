#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod altgr;
mod clipboard;
mod crypto;
mod ui;
mod vault;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;
use zeroize::Zeroize;

use vault::Vault;

const CLIPBOARD_TTL: Duration = Duration::from_secs(30);
const APP_USER_MODEL_ID: &str = "gotylib.passkeep";

#[cfg(windows)]
fn set_app_user_model_id() {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

    let wide: Vec<u16> = OsStr::new(APP_USER_MODEL_ID)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        SetCurrentProcessExplicitAppUserModelID(wide.as_ptr());
    }
}

fn main() -> eframe::Result<()> {
    #[cfg(windows)]
    set_app_user_model_id();

    let viewport = egui::ViewportBuilder::default()
        .with_title("passkeep")
        .with_inner_size([440.0, 680.0])
        .with_min_inner_size([380.0, 520.0])
        .with_icon(ui::icon());

    eframe::run_native(
        "passkeep",
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

struct App {
    path: PathBuf,
    phase: Phase,
}

enum Phase {
    Locked(LockState),
    Open(OpenState),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UnlockStep {
    Idle,
    ShowBusy,
    Run,
}

struct LockState {
    password: String,
    confirm: String,
    show_password: bool,
    error: Option<String>,
    step: UnlockStep,
    creating: bool,
    autofocus: bool,
}

impl Drop for LockState {
    fn drop(&mut self) {
        self.password.zeroize();
        self.confirm.zeroize();
    }
}

struct OpenState {
    session: crypto::Session,
    vault: Vault,
    query: String,
    form: Option<Form>,
    revealed: BTreeSet<String>,
    confirm_delete: Option<String>,
    notice: Option<Notice>,
    copied: Option<Copied>,
    dirty: bool,
    focus_search: bool,
}

impl OpenState {
    fn new(session: crypto::Session, vault: Vault) -> Self {
        Self {
            session,
            vault,
            query: String::new(),
            form: None,
            revealed: BTreeSet::new(),
            confirm_delete: None,
            notice: None,
            copied: None,
            dirty: false,
            focus_search: false,
        }
    }
}

impl Drop for OpenState {
    fn drop(&mut self) {
        if let Some(mut copied) = self.copied.take() {
            let mut secret = std::mem::take(&mut copied.secret);
            drop(copied);
            if !secret.is_empty() {
                clipboard::clear_if_matches(&secret);
            }
            secret.zeroize();
        }
    }
}

struct Form {
    id: Option<String>,
    label: String,
    url: String,
    login: String,
    password: String,
    show_password: bool,
    error: Option<String>,
    focus: bool,
}

impl Form {
    fn blank() -> Self {
        Self {
            id: None,
            label: String::new(),
            url: String::new(),
            login: String::new(),
            password: String::new(),
            show_password: false,
            error: None,
            focus: true,
        }
    }
}

impl Drop for Form {
    fn drop(&mut self) {
        self.label.zeroize();
        self.url.zeroize();
        self.login.zeroize();
        self.password.zeroize();
    }
}

struct Copied {
    secret: String,
    until: Instant,
}

impl Drop for Copied {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

enum Notice {
    Saved,
    Copied(&'static str),
    Error(String),
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        ui::install(&cc.egui_ctx);
        let path = vault::vault_path();
        let creating = !path.exists();
        Self {
            path,
            phase: Phase::Locked(fresh_lock(creating)),
        }
    }

    fn update_frame(&mut self, ctx: &egui::Context) {
        self.poll_clipboard(ctx);

        let run_kdf = matches!(
            &self.phase,
            Phase::Locked(lock) if lock.step == UnlockStep::Run
        );
        if let Phase::Locked(lock) = &mut self.phase {
            if lock.step == UnlockStep::ShowBusy {
                lock.step = UnlockStep::Run;
                ctx.request_repaint();
            }
        }

        ui::show(self, ctx);

        if run_kdf {
            self.finish_pending_unlock();
            ctx.request_repaint();
        }
        self.schedule_clipboard(ctx);
    }

    fn finish_pending_unlock(&mut self) {
        let (password, creating) = {
            let Phase::Locked(lock) = &mut self.phase else {
                return;
            };
            if lock.step != UnlockStep::Run {
                return;
            }
            lock.step = UnlockStep::Idle;
            (std::mem::take(&mut lock.password), lock.creating)
        };

        let mut password = password;
        let result = if creating {
            self.create_vault(&password)
        } else {
            self.unlock(&password)
        };

        match result {
            Ok(()) => password.zeroize(),
            Err(message) => {
                if let Phase::Locked(lock) = &mut self.phase {
                    lock.password = password;
                    lock.error = Some(message);
                    lock.step = UnlockStep::Idle;
                    lock.autofocus = true;
                }
            }
        }
    }

    fn create_vault(&mut self, password: &str) -> Result<(), String> {
        let vault = Vault::empty();
        let (file, session) =
            crypto::create(&vault, password).map_err(|error| error.user_message())?;
        vault::write_atomic(&self.path, &file)
            .map_err(|error| format!("couldn't create the file: {error}"))?;
        self.phase = Phase::Open(OpenState::new(session, vault));
        Ok(())
    }

    fn unlock(&mut self, password: &str) -> Result<(), String> {
        let file = std::fs::read(&self.path)
            .map_err(|error| format!("couldn't read the vault: {error}"))?;
        let (vault, session) =
            crypto::open(&file, password).map_err(|error| error.user_message())?;
        self.phase = Phase::Open(OpenState::new(session, vault));
        Ok(())
    }

    fn save(&mut self) -> Result<(), String> {
        let file = {
            let Phase::Open(open) = &self.phase else {
                return Ok(());
            };
            crypto::seal(&open.vault, &open.session).map_err(|error| error.user_message())?
        };
        vault::write_atomic(&self.path, &file)
            .map_err(|error| format!("couldn't save: {error}"))?;
        if let Phase::Open(open) = &mut self.phase {
            open.dirty = false;
        }
        Ok(())
    }

    fn lock(&mut self) -> Result<(), String> {
        let dirty = matches!(&self.phase, Phase::Open(open) if open.dirty);
        if dirty {
            self.save()?;
        }
        let creating = !self.path.exists();
        self.phase = Phase::Locked(fresh_lock(creating));
        Ok(())
    }

    fn poll_clipboard(&mut self, ctx: &egui::Context) {
        let until = match &self.phase {
            Phase::Open(open) => open.copied.as_ref().map(|copied| copied.until),
            Phase::Locked(_) => None,
        };
        let Some(until) = until else {
            return;
        };
        let now = Instant::now();
        if now < until {
            ctx.request_repaint_after(until.saturating_duration_since(now));
            return;
        }

        let secret = match &mut self.phase {
            Phase::Open(open) => open
                .copied
                .take()
                .map(|mut copied| std::mem::take(&mut copied.secret)),
            Phase::Locked(_) => None,
        };
        let Some(mut secret) = secret else {
            return;
        };
        if !clipboard::clear_if_matches(&secret) {
            if let Phase::Open(open) = &mut self.phase {
                open.copied = Some(Copied {
                    secret,
                    until: Instant::now() + Duration::from_secs(1),
                });
            }
            return;
        }
        secret.zeroize();
        if let Phase::Open(open) = &mut self.phase {
            if matches!(open.notice, Some(Notice::Copied(_))) {
                open.notice = None;
            }
        }
    }

    fn schedule_clipboard(&self, ctx: &egui::Context) {
        let Phase::Open(open) = &self.phase else {
            return;
        };
        let Some(copied) = &open.copied else {
            return;
        };
        let now = Instant::now();
        if copied.until > now {
            ctx.request_repaint_after(copied.until.saturating_duration_since(now));
        }
    }
}

fn fresh_lock(creating: bool) -> LockState {
    LockState {
        password: String::new(),
        confirm: String::new(),
        show_password: false,
        error: None,
        step: UnlockStep::Idle,
        creating,
        autofocus: true,
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [16.0 / 255.0, 16.0 / 255.0, 18.0 / 255.0, 1.0]
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        altgr::recover_special_keys(raw_input);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.update_frame(ctx);
    }
}
