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
    session_id: String,
    client: ApiClient,
    model: String,
    context_window: u64,
    authed: bool,
    auth_error: Option<String>,
    save_error: Option<String>,
}
enum ProviderEvent {
    Phase(&'static str),
    Ready(std::result::Result<Box<ProviderReady>, String>),
}
enum SkillEvent {
    Previous(Vec<(String, String)>),
    Ready(PathBuf, std::result::Result<Vec<(String, String)>, String>),
}

pub(super) struct StartupState {
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
        Self {
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
        self.start_provider_preparation();
        self.refresh_skill_palette_cache();
    }

    fn start_provider_preparation(&mut self) {
        let Some(mut session) = self.session.as_deref().cloned() else {
            return;
        };
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
                    let _ = tx.send(ProviderEvent::Phase("saving session"));
                    session.model = cfg.model.clone();
                    session.provider = cfg.provider.clone();
                    let save_error = session.save().err().map(|e| e.to_string());
                    crate::ade::write_ade_manifest(
                        &session.id,
                        &cfg.model,
                        &session.cwd,
                        &session.usage,
                        "idle",
                    );
                    crate::startup::mark("session_saved");
                    Ok(Box::new(ProviderReady {
                        selection,
                        session_id: session.id,
                        client,
                        model: cfg.model,
                        context_window: cfg.context_window,
                        authed,
                        auth_error,
                        save_error,
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

    pub(super) fn poll_startup(&mut self) -> bool {
        let was_pending = self.startup_pending();
        let mut dirty = false;
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
                            if ready.selection != Selection::from_config(&self.cfg)
                                || ready.session_id != self.session_id
                            {
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
                            if let Some(error) = ready.save_error {
                                self.push_error(format!("session save: {error}"));
                            }
                            if let Some(error) = ready.auth_error {
                                self.push_error(error);
                            }
                        }
                        Err(error) => {
                            self.authed = false;
                            self.push_error(error);
                        }
                    }
                    if !self.authed {
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
            if !self.busy && self.authed && self.login.is_none() && self.theme_picker.is_none() {
                if let Some(next) = self.queue.pop_front() {
                    if let Some(index) = self
                        .cells
                        .iter()
                        .position(|c| matches!(c, Cell::Queued { text } if text == &next.text))
                    {
                        self.remove_cell(index);
                    }
                    self.submit_queued(next);
                    dirty = true;
                }
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
        dirty
    }

    pub(super) fn queue_during_startup(&mut self, text: &str) {
        let images = self.take_draft_images();
        self.queue.push_back(QueuedPrompt {
            text: text.to_string(),
            images,
            raw_submission: true,
        });
        self.cells.push(Cell::Queued {
            text: text.to_string(),
        });
        self.scroll_to_bottom();
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

    pub(super) fn submit_queued(&mut self, next: QueuedPrompt) {
        if self.startup_pending() {
            self.cells.push(Cell::Queued {
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
