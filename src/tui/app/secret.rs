//! The secret modal: a name and a masked value, saved to `crate::secrets` so
//! the value never enters the prompt, the session or the transcript.
//!
//! It opens three ways: `/secrets add [NAME]`, a credential typed or pasted
//! into the prompt (the message waits and is sent with `$NAME` in its place),
//! and the agent's `secret` request (answered with the name only).
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretField {
    Name,
    Value,
}

/// Where the modal came from, and so what saving it does next.
pub enum SecretOrigin {
    /// `/secrets add`: save, then say so.
    Command,
    /// A credential in a message being sent: save it, then send the message
    /// with `$NAME` where the value was. Cancelling puts the message back in
    /// the editor unsent.
    Pasted {
        prompt: String,
        range: std::ops::Range<usize>,
        kind: &'static str,
    },
    /// The agent's `secret` request: answered with the saved name, or `None`.
    Request {
        purpose: String,
        respond: Option<oneshot::Sender<Option<String>>>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct SecretHit {
    pub frame: ratatui::layout::Rect,
    pub close: ratatui::layout::Rect,
    pub name_row: ratatui::layout::Rect,
    pub value_row: ratatui::layout::Rect,
}

pub struct SecretModal {
    pub name: String,
    pub value: String,
    pub field: SecretField,
    pub origin: SecretOrigin,
    pub error: Option<String>,
    pub hit: SecretHit,
}

impl SecretModal {
    /// Whether saving replaces a secret already stored under this name.
    pub fn replaces(&self) -> bool {
        crate::secrets::valid_name(&self.name) && crate::secrets::exists(&self.name)
    }

    fn field_mut(&mut self) -> &mut String {
        match self.field {
            SecretField::Name => &mut self.name,
            SecretField::Value => &mut self.value,
        }
    }

    /// Type into the focused field. A name only takes what an environment
    /// variable can hold, upper-cased as it is typed.
    fn type_text(&mut self, text: &str) {
        match self.field {
            SecretField::Name => {
                let clean = crate::secrets::normalize_name(text);
                self.name.extend(
                    clean
                        .chars()
                        .filter(|c| c.is_ascii_alphanumeric() || *c == '_'),
                );
                self.name.truncate(64);
            }
            SecretField::Value => self.value.push_str(text.trim_matches(['\r', '\n'])),
        }
        self.error = None;
    }
}

impl App {
    /// Open the modal for `/secrets add [NAME]`.
    pub(super) fn open_secret_modal(&mut self, name: &str) {
        let name = crate::secrets::normalize_name(name);
        let field = if name.is_empty() {
            SecretField::Name
        } else {
            SecretField::Value
        };
        self.secret = Some(SecretModal {
            name,
            value: String::new(),
            field,
            origin: SecretOrigin::Command,
            error: None,
            hit: SecretHit::default(),
        });
    }

    /// The agent asked for a credential (`secret` action request).
    pub(super) fn open_secret_request(
        &mut self,
        name: String,
        purpose: String,
        respond: oneshot::Sender<Option<String>>,
    ) {
        // One modal at a time: a second request while one is open is declined.
        if self.secret.is_some() {
            let _ = respond.send(None);
            return;
        }
        self.secret = Some(SecretModal {
            name,
            value: String::new(),
            field: SecretField::Value,
            origin: SecretOrigin::Request {
                purpose,
                respond: Some(respond),
            },
            error: None,
            hit: SecretHit::default(),
        });
    }

    /// A message about to be sent holds a live-looking credential: hold the
    /// message and open the modal on it instead. Returns true when it did.
    pub(super) fn catch_pasted_secret(&mut self, text: &str) -> bool {
        if !self.cfg.secrets.catch_pasted || self.secret.is_some() {
            return false;
        }
        let Some(found) = crate::tools::sensitive::live_tokens(text)
            .into_iter()
            .next()
        else {
            return false;
        };
        self.secret = Some(SecretModal {
            name: crate::secrets::free_name(found.env_name),
            value: text[found.range.clone()].to_string(),
            field: SecretField::Name,
            origin: SecretOrigin::Pasted {
                prompt: text.to_string(),
                range: found.range,
                kind: found.kind,
            },
            error: None,
            hit: SecretHit::default(),
        });
        true
    }

    pub(super) fn on_secret_key(&mut self, key: event::KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let paste = (key.code == KeyCode::Char('v') && ctrl)
            || (key.code == KeyCode::Insert && key.modifiers.contains(KeyModifiers::SHIFT));
        if paste {
            if let Some(text) = clipboard_get() {
                self.on_secret_paste(&text);
            }
            return;
        }
        let Some(m) = &mut self.secret else { return };
        match key.code {
            KeyCode::Esc => self.close_secret(false),
            KeyCode::Enter => {
                if m.field == SecretField::Name && m.value.is_empty() {
                    m.field = SecretField::Value;
                } else {
                    self.save_secret();
                }
            }
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => {
                m.field = match m.field {
                    SecretField::Name => SecretField::Value,
                    SecretField::Value => SecretField::Name,
                };
            }
            KeyCode::Backspace => {
                m.field_mut().pop();
                m.error = None;
            }
            KeyCode::Char('u') if ctrl => {
                m.field_mut().clear();
                m.error = None;
            }
            KeyCode::Char(c) if !ctrl && !c.is_control() => m.type_text(&c.to_string()),
            _ => {}
        }
    }

    pub(super) fn on_secret_paste(&mut self, text: &str) {
        if let Some(m) = &mut self.secret {
            m.type_text(text.trim());
        }
    }

    pub(super) fn on_secret_mouse(&mut self, m: event::MouseEvent) {
        self.mouse_col = m.column;
        self.mouse_row = m.row;
        let MouseEventKind::Down(MouseButton::Left) = m.kind else {
            return;
        };
        let Some(modal) = &mut self.secret else {
            return;
        };
        let hit = modal.hit.clone();
        if rect_contains(hit.close, m.column, m.row) || !rect_contains(hit.frame, m.column, m.row) {
            self.close_secret(false);
        } else if rect_contains(hit.name_row, m.column, m.row) {
            modal.field = SecretField::Name;
        } else if rect_contains(hit.value_row, m.column, m.row) {
            modal.field = SecretField::Value;
        }
    }

    fn save_secret(&mut self) {
        let Some(m) = &mut self.secret else { return };
        let name = m.name.clone();
        if let Err(error) = crate::secrets::set(&name, &m.value) {
            m.error = Some(error);
            return;
        }
        self.close_secret(true);
    }

    /// Close the modal: `saved` after a successful save, else cancelled.
    pub(super) fn close_secret(&mut self, saved: bool) {
        let Some(mut modal) = self.secret.take() else {
            return;
        };
        self.transcript_revision = self.transcript_revision.wrapping_add(1);
        let name = modal.name.clone();
        match &mut modal.origin {
            SecretOrigin::Command => {
                if saved {
                    self.push_note(
                        Tone::Session,
                        format!("saved ${name} · set in every shell command"),
                    );
                }
            }
            SecretOrigin::Request { respond, .. } => {
                if let Some(respond) = respond.take() {
                    let _ = respond.send(saved.then(|| name.clone()));
                }
                if saved {
                    self.push_note(Tone::Session, format!("saved ${name} for the agent"));
                }
            }
            SecretOrigin::Pasted { prompt, range, .. } => {
                if saved {
                    let mut sent = prompt.clone();
                    sent.replace_range(range.clone(), &format!("${name}"));
                    self.push_note(
                        Tone::Session,
                        format!("saved ${name} · your message carries the name, not the value"),
                    );
                    self.submit_text(&sent);
                } else {
                    // Back to the editor, unsent, exactly as typed.
                    self.input.insert_str(prompt);
                }
            }
        }
    }

    /// The turn that asked for a secret ended: its answer can no longer be
    /// read, so the modal goes with it.
    pub(super) fn drop_secret_request(&mut self) {
        if matches!(
            self.secret.as_ref().map(|m| &m.origin),
            Some(SecretOrigin::Request { .. })
        ) {
            self.secret = None;
        }
    }

    /// `/secrets [list | add [NAME] | remove NAME]`.
    pub(super) fn cmd_secrets(&mut self, arg: &str) {
        let mut words = arg.split_whitespace();
        match (words.next(), words.next()) {
            (None | Some("list"), _) => {
                let names = crate::secrets::names();
                if names.is_empty() {
                    self.push_info("no secrets saved · /secrets add <NAME>".into());
                } else {
                    let rows: Vec<String> = names
                        .iter()
                        .map(|(name, added)| {
                            format!("${name}  {}", added.get(..10).unwrap_or(added))
                        })
                        .collect();
                    self.push_note(
                        Tone::Session,
                        format!("secrets · {}\n{}", names.len(), rows.join("\n")),
                    );
                }
            }
            (Some("add" | "set" | "new"), name) => self.open_secret_modal(name.unwrap_or("")),
            (Some("remove" | "rm" | "delete"), Some(name)) => {
                let name = crate::secrets::normalize_name(name);
                match crate::secrets::remove(&name) {
                    Ok(true) => self.push_info(format!("removed ${name}")),
                    Ok(false) => self.push_error(format!("no secret named ${name}")),
                    Err(e) => self.push_error(format!("could not remove ${name}: {e}")),
                }
            }
            _ => self.push_error("usage: /secrets [list | add [NAME] | remove NAME]".into()),
        }
    }
}
