//! Interactive preparation starts only after the first frame. The editor and
//! queued drafts remain live while providers, skills, and durable state load.
use super::*;

#[derive(Clone, PartialEq, Eq)]
struct Selection {
    provider: String,
    model: String,
    base_url: String,
}
impl Selection {
    fn from_config(cfg: &Config) -> Self {
        Self {
            provider: cfg.provider.clone(),
            model: cfg.model.clone(),
            base_url: cfg.base_url.clone(),
        }
    }
}
struct ProviderReady {
    selection: Selection,
    client: ApiClient,
    model: String,
    context_window: u64,
    authed: bool,
    auth_error: Option<String>,
}
enum ProviderEvent {
    Phase(&'static str),
    Ready(std::result::Result<Box<ProviderReady>, String>),
}
enum SkillEvent {
    Previous(Vec<(String, String)>),
    Ready(PathBuf, std::result::Result<Vec<(String, String)>, String>),
}

type AccountApply = Box<dyn FnOnce(&mut App) + Send>;
type AccountJob = Box<dyn FnOnce() -> (u64, AccountApply) + Send>;

pub(super) struct StartupState {
    ui_tx: mpsc::UnboundedSender<AccountApply>,
    ui_rx: mpsc::UnboundedReceiver<AccountApply>,
    pub(super) login_epoch: u64,
    account_tx: Option<std::sync::mpsc::Sender<AccountJob>>,
    account_rx: Option<mpsc::UnboundedReceiver<(u64, AccountApply)>>,
    account_epoch: u64,
    account_pending: usize,
    launched: bool,
    provider_ready: bool,
    skills_ready: bool,
    provider_phase: &'static str,
    provider_rx: Option<mpsc::UnboundedReceiver<ProviderEvent>>,
    skills_rx: Option<mpsc::UnboundedReceiver<SkillEvent>>,
    last_check: Instant,
    started: Instant,
    announced_ready: bool,
    maintenance_started: bool,
    pub(super) list_skills: bool,
}
impl StartupState {
    pub(super) fn new() -> Self {
        let (ui_tx, ui_rx) = mpsc::unbounded_channel();
        Self {
            ui_tx,
            ui_rx,
            login_epoch: 0,
            account_tx: None,
            account_rx: None,
            account_epoch: 0,
            account_pending: 0,
            launched: false,
            provider_ready: false,
            skills_ready: false,
            provider_phase: "loading account",
            provider_rx: None,
            skills_rx: None,
            last_check: Instant::now(),
            started: Instant::now(),
            announced_ready: false,
            maintenance_started: false,
            list_skills: false,
        }
    }
    pub(super) fn pending(&self) -> bool {
        !self.provider_ready || !self.skills_ready
    }
}

impl App {
    pub fn startup_pending(&self) -> bool {
        self.startup.pending()
    }

    pub(super) fn launch_startup(&mut self) {
        if self.startup.launched {
            return;
        }
        self.startup.launched = true;
        crate::startup::mark("first_frame");
        #[cfg(feature = "image-peek")]
        {
            let cfg = self.cfg.clone();
            self.background_ui(
                move || super::prepare_image_picker(&cfg),
                |app, picker| {
                    app.img_picker = Some(picker);
                },
            );
        }
        self.start_provider_preparation();
        self.refresh_skill_palette_cache();
    }

    pub(super) fn restart_provider_preparation(&mut self) {
        self.startup.account_epoch += 1;
        self.start_provider_preparation();
    }

    fn start_provider_preparation(&mut self) {
        let mut cfg = self.cfg.clone();
        let selection = Selection::from_config(&cfg);
        let (tx, rx) = mpsc::unbounded_channel();
        self.startup.provider_rx = Some(rx);
        self.startup.provider_ready = false;
        let failure = tx.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("nur-startup-provider".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    crate::pricing::prepare_startup(&mut cfg);
                    crate::startup::mark("catalog_ready");
                    let _ = tx.send(ProviderEvent::Phase("connecting account"));
                    let (key, auth_error) =
                        match crate::auth::resolve_api_key_for(Some(&cfg.provider)) {
                            Ok(key) => (key, None),
                            Err(error) => (String::new(), Some(error.to_string())),
                        };
                    crate::startup::mark("credentials_ready");
                    let provider = crate::providers::by_id(&cfg.provider)
                        .copied()
                        .unwrap_or(*crate::providers::default_provider());
                    let authed = auth_error.is_none() && (!key.is_empty() || provider.key_optional);
                    if crate::providers::is_placeholder_local_model(&cfg.model) {
                        let _ = tx.send(ProviderEvent::Phase("loading local models"));
                        cfg.model = crate::api::models::resolve_local_model_if_needed(
                            &cfg.base_url,
                            &cfg.provider,
                            &key,
                            &cfg.model,
                        );
                        crate::pricing::maybe_apply_context_window(&mut cfg);
                    }
                    let client = ApiClient::for_provider(&cfg.base_url, &key, &cfg.provider)
                        .map_err(|e| e.to_string())?
                        .with_style(provider.style);
                    Ok(Box::new(ProviderReady {
                        selection,
                        client,
                        model: cfg.model,
                        context_window: cfg.context_window,
                        authed,
                        auth_error,
                    }))
                }))
                .unwrap_or_else(|_| {
                    Err("provider preparation interrupted; use /login to reconnect".into())
                });
                let _ = tx.send(ProviderEvent::Ready(result));
            })
        {
            let _ = failure.send(ProviderEvent::Ready(Err(error.to_string())));
        }
    }

    /// All callers, including /cd and plugin refresh, avoid filesystem scans
    /// on the event thread. Results are checked against the current workspace.
    pub fn refresh_skill_palette_cache(&mut self) {
        if self.startup.skills_rx.is_some() {
            return;
        }
        self.startup.skills_ready = false;
        let cwd = self.cwd.clone();
        let failed_cwd = cwd.clone();
        let (tx, rx) = mpsc::unbounded_channel();
        self.startup.skills_rx = Some(rx);
        let failure = tx.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("nur-startup-skills".into())
            .spawn(move || {
                let _ = tx.send(SkillEvent::Previous(agent::skill_cache::cached_palette()));
                let result = std::panic::catch_unwind(|| {
                    agent::skills::load_skills(&cwd)
                        .into_iter()
                        .map(|sk| (sk.name, sk.description.chars().take(72).collect()))
                        .collect()
                })
                .map_err(|_| "skill indexing interrupted; retrying discovery".to_string());
                crate::startup::mark("skills_ready");
                let _ = tx.send(SkillEvent::Ready(cwd, result));
            })
        {
            let _ = failure.send(SkillEvent::Ready(failed_cwd, Err(error.to_string())));
        }
    }

    /// One-off blocking OS work returns a UI completion without holding up input.
    pub(super) fn background_ui<R: Send + 'static>(
        &mut self,
        operation: impl FnOnce() -> R + Send + 'static,
        apply: impl FnOnce(&mut App, R) + Send + 'static,
    ) {
        let tx = self.startup.ui_tx.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("nur-ui-work".into())
            .spawn(move || {
                let result = operation();
                let _ = tx.send(Box::new(move |app| apply(app, result)));
            })
        {
            self.push_error(format!("background operation: {error}"));
        }
    }

    /// Credential writes/resolution are serialized off the terminal thread.
    /// Only the latest UI intent can install a client or finish a login modal.
    pub(super) fn account_work<R: Send + 'static>(
        &mut self,
        affects_provider: bool,
        operation: impl FnOnce() -> R + Send + 'static,
        apply: impl FnOnce(&mut App, R) + Send + 'static,
    ) {
        if self.startup.account_tx.is_none() {
            let (tx, jobs) = std::sync::mpsc::channel::<AccountJob>();
            let (done, rx) = mpsc::unbounded_channel();
            match std::thread::Builder::new()
                .name("nur-accounts".into())
                .spawn(move || {
                    while let Ok(job) = jobs.recv() {
                        let completion = job();
                        let _ = done.send(completion);
                    }
                }) {
                Ok(_) => {
                    self.startup.account_tx = Some(tx);
                    self.startup.account_rx = Some(rx);
                }
                Err(error) => {
                    self.push_error(format!("account worker: {error}"));
                    return;
                }
            }
        }
        self.startup.account_epoch += 1;
        let epoch = self.startup.account_epoch;
        let modal_epoch = self.login.as_ref().map(|_| self.startup.login_epoch);
        let job = Box::new(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation));
            let apply: AccountApply = match result {
                Ok(result) => Box::new(move |app| {
                    if modal_epoch.is_none_or(|epoch| epoch == app.startup.login_epoch) {
                        apply(app, result);
                    }
                }),
                Err(_) => Box::new(|app| {
                    app.authed = false;
                    app.startup.provider_ready = true;
                    app.push_error("account operation interrupted".into());
                }),
            };
            (epoch, apply)
        });
        if self.startup.account_tx.as_ref().unwrap().send(job).is_err() {
            self.startup.account_tx = None;
            self.push_error("account worker stopped".into());
            return;
        }
        self.startup.account_pending += 1;
        if affects_provider {
            self.startup.provider_rx = None;
            self.startup.provider_ready = false;
            self.startup.provider_phase = "updating account";
        }
    }

    pub(super) fn poll_startup(&mut self) -> bool {
        let was_pending = self.startup_pending();
        let mut dirty = false;
        while let Ok(apply) = self.startup.ui_rx.try_recv() {
            apply(self);
            dirty = true;
        }
        while let Some((epoch, apply)) = self
            .startup
            .account_rx
            .as_mut()
            .and_then(|rx| rx.try_recv().ok())
        {
            self.startup.account_pending = self.startup.account_pending.saturating_sub(1);
            if epoch == self.startup.account_epoch {
                apply(self);
            }
            dirty = true;
        }
        if self.startup.account_pending == 0
            && !self.startup.provider_ready
            && self.startup.provider_rx.is_none()
            && self.startup.launched
        {
            self.start_provider_preparation();
        }
        while let Some(event) = self
            .startup
            .provider_rx
            .as_mut()
            .and_then(|rx| rx.try_recv().ok())
        {
            dirty = true;
            match event {
                ProviderEvent::Phase(phase) => self.startup.provider_phase = phase,
                ProviderEvent::Ready(result) => {
                    self.startup.provider_rx = None;
                    self.startup.provider_ready = true;
                    match result {
                        Ok(ready) => {
                            if ready.selection != Selection::from_config(&self.cfg) {
                                // A user-selected model/provider always wins over
                                // an older in-flight startup result.
                                self.start_provider_preparation();
                                continue;
                            }
                            self.client = ready.client;
                            self.cfg.model = ready.model;
                            self.cfg.context_window = ready.context_window;
                            self.authed = ready.authed;
                            if let Some(s) = self.session.as_mut() {
                                s.model = self.cfg.model.clone();
                                s.provider = self.cfg.provider.clone();
                            }
                            if let Some(u) = self.usage.as_mut() {
                                u.set_model(self.cfg.model.clone());
                                u.set_provider(self.cfg.provider.clone());
                            }
                            std::env::set_var("NUR_MODEL", &self.cfg.model);
                            self.save_prepared_session();
                            if let Some(error) = ready.auth_error {
                                self.push_error(error);
                            }
                        }
                        Err(error) => {
                            self.authed = false;
                            self.push_error(error);
                        }
                    }
                    if !self.authed && self.login.is_none() && self.theme_picker.is_none() {
                        if self.cfg.theme.is_none() {
                            self.open_theme_picker(true);
                        } else {
                            self.open_login();
                        }
                    }
                }
            }
        }
        while let Some(event) = self
            .startup
            .skills_rx
            .as_mut()
            .and_then(|rx| rx.try_recv().ok())
        {
            dirty = true;
            match event {
                SkillEvent::Previous(palette) => {
                    if self.skill_palette_cache.is_empty() {
                        self.skill_palette_cache = palette;
                    }
                }
                SkillEvent::Ready(cwd, result) => {
                    self.startup.skills_rx = None;
                    if cwd != self.cwd {
                        self.refresh_skill_palette_cache();
                        continue;
                    }
                    match result {
                        Ok(palette) => {
                            self.skill_palette_cache = palette;
                            self.startup.skills_ready = true;
                            if self.startup.list_skills {
                                self.startup.list_skills = false;
                                self.show_skill_listing();
                            }
                        }
                        Err(error) => self.push_error(error),
                    }
                }
            }
        }
        if self.startup.launched && self.startup.last_check.elapsed() >= Duration::from_secs(2) {
            self.startup.last_check = Instant::now();
            // Checking validity is cheap; rebuilding is always on a worker.
            if self.startup.skills_rx.is_none() && !agent::skill_cache::is_current() {
                self.refresh_skill_palette_cache();
                dirty = true;
            }
        }
        if self.startup_pending() {
            let status = match (self.startup.provider_ready, self.startup.skills_ready) {
                (false, false) => format!("{} · indexing skills", self.startup.provider_phase),
                (false, true) => self.startup.provider_phase.to_string(),
                _ => "indexing skills".into(),
            };
            if !self.busy && self.status != status {
                self.status = status;
                dirty = true;
            }
        } else {
            if was_pending && !self.busy {
                self.status = "idle".into();
                dirty = true;
            }
            if !self.startup.announced_ready {
                self.startup.announced_ready = true;
                crate::startup::mark("ready");
                if !self.title_from_prompt {
                    crate::ade::set_title_prompt("ready");
                }
                if !self.busy {
                    self.status = "idle".into();
                }
                dirty = true;
            }
            if !self.startup.maintenance_started
                && !self.busy
                && self.startup.started.elapsed() >= Duration::from_secs(2)
            {
                self.startup.maintenance_started = true;
                let update = self.cfg.auto_update;
                let repair = self.cfg.ecosystem_auto_ensure;
                std::thread::spawn(move || {
                    crate::bootstrap::maybe_auto_update_on_launch(update);
                    if repair && crate::bootstrap::should_repair_ecosystem() {
                        let _ = crate::ecosystem::ensure_ecosystem(false);
                    }
                });
            }
        }
        if !self.busy
            && self.startup.account_pending == 0
            && self.login.is_none()
            && self.theme_picker.is_none()
        {
            let runnable = self.queue.iter().position(|next| {
                self.startup.provider_ready
                    && (!next.wait_for_skills || self.startup.skills_ready)
                    && (self.authed || next.raw_submission && next.text.starts_with('/'))
            });
            if let Some(index) = runnable {
                let next = self.queue.remove(index).unwrap();
                if let Some(index) = self
                    .cells
                    .iter()
                    .position(|c| matches!(c, Cell::Queued { id, .. } if *id == next.id))
                {
                    self.remove_cell(index);
                }
                self.submit_queued(next);
                dirty = true;
            }
        }
        dirty
    }

    /// Local controls never call this. Model turns need both dependencies;
    /// compaction only needs the provider, and does not discover skills.
    pub(super) fn defer_until_prepared(&mut self, text: &str, skills: bool) -> bool {
        self.transcript_revision = self.transcript_revision.wrapping_add(1);
        if skills && !agent::skill_cache::is_current() {
            self.refresh_skill_palette_cache();
        }
        if self.startup.provider_ready && (!skills || self.startup.skills_ready) {
            return false;
        }
        let images = self.take_draft_images();
        let mut queued = QueuedPrompt::new(text.to_string(), &self.session_id, &self.cwd);
        queued.images = images;
        queued.raw_submission = true;
        queued.wait_for_skills = skills;
        self.cells.push(Cell::Queued {
            id: queued.id,
            text: text.to_string(),
        });
        self.queue.push_back(queued);
        self.scroll_to_bottom();
        true
    }

    /// Login/model selection is authoritative even when the same provider is
    /// selected again. Dropping the receiver prevents stale credentials from
    /// replacing the newly selected client. Workers never write session state.
    pub(super) fn provider_selected(&mut self) {
        self.startup.provider_rx = None;
        self.startup.provider_ready = true;
        self.save_prepared_session();
    }

    fn save_prepared_session(&mut self) {
        if let Some(session) = self.session.as_ref() {
            let error = session.save().err();
            crate::ade::write_ade_manifest(
                &session.id,
                &session.model,
                &session.cwd,
                &session.usage,
                "idle",
            );
            if let Some(error) = error {
                self.push_error(format!("session save: {error}"));
            }
        }
        crate::startup::mark("session_saved");
    }

    fn show_skill_listing(&mut self) {
        let mut text = String::from("skills - invoke with /name (sticky) or /name <prompt> (one-shot)\nnatural-language phrases also activate skills\n");
        for (name, description) in self.skill_palette_cache.iter().take(80) {
            text.push_str(&format!(
                "  /{name} - {}\n",
                description.chars().take(64).collect::<String>()
            ));
        }
        if self.skill_palette_cache.len() > 80 {
            text.push_str(&format!(
                "  ... +{} more (type /partial-name to filter in the palette)\n",
                self.skill_palette_cache.len() - 80
            ));
        }
        if self.skill_palette_cache.is_empty() {
            text.push_str("no skills found - add ~/.nur/skills/<name>/SKILL.md\n");
        }
        if !self.sticky_skills.is_empty() {
            text.push_str(&format!(
                "\nsticky this session: {}\n",
                self.sticky_skills.join(", ")
            ));
        }
        self.push_note(Tone::Skill, text);
    }

    pub(super) fn discard_pending_requests(&mut self) {
        self.queue.clear();
        self.remove_cells_matching(|cell| matches!(cell, Cell::Queued { .. }));
        self.preserve_queue_on_interrupt = false;
    }

    pub(super) fn submit_queued(&mut self, next: QueuedPrompt) {
        self.transcript_revision = self.transcript_revision.wrapping_add(1);
        if next.session_id != self.session_id || next.cwd != self.cwd {
            return;
        }
        if !self.startup.provider_ready || next.wait_for_skills && !self.startup.skills_ready {
            self.cells.push(Cell::Queued {
                id: next.id,
                text: next.text.clone(),
            });
            self.queue.push_front(next);
            return;
        }
        if next.raw_submission {
            // An unsubmitted draft's attachments belong to that draft, not to
            // the earlier queued request now being dispatched.
            let draft_images = self.take_draft_images();
            for (path, label) in next.images {
                self.cells.push(Cell::Image {
                    path,
                    label,
                    queued: true,
                });
            }
            self.submit_text(&next.text);
            for (path, label) in draft_images {
                self.cells.push(Cell::Image {
                    path,
                    label,
                    queued: true,
                });
            }
        } else {
            self.start_attached_turn(&next.text, next.images);
        }
    }
}
