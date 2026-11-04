//! A session-scoped Debug Adapter Protocol client over stdio.
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex, MutexGuard, OnceLock, TryLockError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use super::{arg_str, Tool, ToolContext};
use crate::error::{NurError, Result};
use crate::process_tree::ProcessTree;

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_MESSAGE: usize = 2 * 1024 * 1024;
type WriteRequest = (Vec<u8>, mpsc::SyncSender<std::result::Result<(), String>>);
type Slot = Arc<Mutex<Option<DebugSession>>>;
type Registry = HashMap<(String, String), Slot>;
static SESSIONS: OnceLock<Mutex<Registry>> = OnceLock::new();
fn sessions() -> &'static Mutex<Registry> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}
fn fail(message: impl Into<String>) -> NurError {
    NurError::Tool(format!("debug: {}", message.into()))
}

struct DebugSession {
    child: Child,
    tree: ProcessTree,
    writer: mpsc::Sender<WriteRequest>,
    incoming: mpsc::Receiver<std::result::Result<Value, String>>,
    next_seq: i64,
    pending: HashMap<i64, Value>,
    outstanding: HashSet<i64>,
    events: VecDeque<Value>,
    closed: Option<String>,
}

impl Drop for DebugSession {
    fn drop(&mut self) {
        self.tree.terminate(&mut self.child);
    }
}

fn read_adapter(
    stdout: std::process::ChildStdout,
) -> mpsc::Receiver<std::result::Result<Value, String>> {
    let (tx, rx) = mpsc::sync_channel(16);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let result = read_message(&mut reader);
            let stop = result.is_err();
            if tx.send(result).is_err() || stop {
                break;
            }
        }
    });
    rx
}

fn read_message(reader: &mut impl BufRead) -> std::result::Result<Value, String> {
    let mut content_length = None;
    let mut total = 0;
    loop {
        let mut line = Vec::new();
        let count = reader
            .take(8193)
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?;
        if count == 0 {
            return Err("adapter closed its output".into());
        }
        total += count;
        if total > 8192 {
            return Err("adapter headers too large".into());
        }
        if line == b"\r\n" || line == b"\n" {
            break;
        }
        let text = std::str::from_utf8(&line).map_err(|_| "invalid adapter header")?;
        if let Some((name, value)) = text.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                if content_length.is_some() {
                    return Err("duplicate content length".into());
                }
                content_length = Some(
                    value
                        .trim()
                        .parse::<usize>()
                        .map_err(|_| "invalid content length")?,
                );
            }
        }
    }
    let length = content_length.ok_or("missing content length")?;
    if length > MAX_MESSAGE {
        return Err("adapter message too large".into());
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    serde_json::from_slice(&body).map_err(|e| format!("invalid adapter JSON: {e}"))
}

impl DebugSession {
    fn spawn(adapter: &str, args: &[String], cwd: &std::path::Path) -> Result<Self> {
        let config = crate::config::load_config().ok();
        let mode = config
            .as_ref()
            .map(|c| crate::sandbox_os::Mode::parse(&c.sandbox.mode))
            .unwrap_or_else(|| {
                crate::sandbox_os::Mode::parse(&crate::config::SandboxConfig::default().mode)
            });
        let enabled = config.as_ref().map(|c| c.sandbox.enabled).unwrap_or(true);
        let argv: Vec<std::ffi::OsString> = std::iter::once(adapter.into())
            .chain(args.iter().map(std::ffi::OsString::from))
            .collect();
        let wrapped = crate::sandbox_os::wrap(&argv, cwd, mode, enabled);
        if let Some(failure) = wrapped.failure {
            return Err(fail(failure));
        }
        let selected = if wrapped.argv.is_empty() {
            &argv
        } else {
            &wrapped.argv
        };
        let mut command = Command::new(&selected[0]);
        crate::tools::tool_env(&mut command);
        command
            .args(&selected[1..])
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let (mut child, tree) =
            ProcessTree::spawn_with_flags(&mut command, crate::process_tree::CREATE_NO_WINDOW)
                .map_err(|e| fail(format!("cannot start adapter: {e}")))?;
        let Some(stdin) = child.stdin.take() else {
            tree.terminate(&mut child);
            return Err(fail("adapter has no stdin"));
        };
        let Some(stdout) = child.stdout.take() else {
            tree.terminate(&mut child);
            return Err(fail("adapter has no stdout"));
        };
        let (writer, requests) = mpsc::channel::<WriteRequest>();
        std::thread::spawn(move || {
            let mut stdin = stdin;
            while let Ok((bytes, reply)) = requests.recv() {
                let result = stdin
                    .write_all(&bytes)
                    .and_then(|_| stdin.flush())
                    .map_err(|error| error.to_string());
                let failed = result.is_err();
                let _ = reply.send(result);
                if failed {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            tree,
            writer,
            incoming: read_adapter(stdout),
            next_seq: 1,
            pending: HashMap::new(),
            outstanding: HashSet::new(),
            events: VecDeque::new(),
            closed: None,
        })
    }
    fn send(&mut self, mut message: Value, cancel: &CancellationToken) -> Result<()> {
        if cancel.is_cancelled() {
            return Err(fail("cancelled"));
        }
        message["seq"] = json!(self.next_seq);
        self.next_seq += 1;
        let body = serde_json::to_vec(&message).map_err(|e| fail(e.to_string()))?;
        if body.len() > MAX_MESSAGE {
            return Err(fail("adapter request too large"));
        }
        let mut packet = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        packet.extend_from_slice(&body);
        let (tx, rx) = mpsc::sync_channel(1);
        self.writer
            .send((packet, tx))
            .map_err(|_| fail("adapter input closed"))?;
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if cancel.is_cancelled() {
                return Err(fail("cancelled"));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(fail("adapter write timed out"));
            }
            match rx.recv_timeout(remaining.min(Duration::from_millis(100))) {
                Ok(result) => {
                    return result.map_err(|e| fail(format!("adapter write failed: {e}")))
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(fail("adapter input closed"))
                }
            }
        }
    }
    fn request(
        &mut self,
        command: &str,
        arguments: Value,
        cancel: &CancellationToken,
    ) -> Result<i64> {
        let id = self.next_seq;
        self.send(
            json!({"type":"request", "command": command, "arguments": arguments}),
            cancel,
        )?;
        self.outstanding.insert(id);
        Ok(id)
    }
    fn accept(&mut self, message: Value, cancel: &CancellationToken) -> Result<()> {
        match message.get("type").and_then(Value::as_str) {
            Some("response") => {
                if let Some(id) = message.get("request_seq").and_then(Value::as_i64) {
                    if self.outstanding.contains(&id) {
                        self.pending.insert(id, message);
                    }
                }
            }
            Some("event") => {
                if self.events.len() == 64 {
                    self.events.pop_front();
                }
                self.events.push_back(message);
            }
            Some("request") => {
                let id = message.get("seq").cloned().unwrap_or(Value::Null);
                let command = message.get("command").cloned().unwrap_or(Value::Null);
                self.send(json!({"type":"response","request_seq":id,"command":command,"success":false,"message":"Client requests are not supported"}), cancel)?;
            }
            _ => {}
        }
        Ok(())
    }
    fn receive(&mut self, deadline: Instant, cancel: &CancellationToken) -> Result<()> {
        loop {
            if cancel.is_cancelled() {
                return Err(fail("cancelled"));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(fail("adapter timed out"));
            }
            match self
                .incoming
                .recv_timeout(remaining.min(Duration::from_millis(100)))
            {
                Ok(Ok(message)) => return self.accept(message, cancel),
                Ok(Err(error)) => return Err(fail(error)),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(fail("adapter closed its output"))
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
            }
        }
    }
    fn response(
        &mut self,
        id: i64,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(response) = self.pending.remove(&id) {
                self.outstanding.remove(&id);
                if response.get("success").and_then(Value::as_bool) != Some(true) {
                    return Err(fail(
                        response
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("adapter request failed"),
                    ));
                }
                return Ok(response.get("body").cloned().unwrap_or_else(|| json!({})));
            }
            self.receive(deadline, cancel)?;
        }
    }
    fn wait_initialized(&mut self, launch_id: i64, cancel: &CancellationToken) -> Result<()> {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if self.pending.get(&launch_id).is_some_and(|response| {
                response.get("success").and_then(Value::as_bool) == Some(false)
            }) {
                self.response(launch_id, TIMEOUT, cancel)?;
            }
            if self
                .events
                .iter()
                .any(|event| event.get("event").and_then(Value::as_str) == Some("initialized"))
            {
                return Ok(());
            }
            self.receive(deadline, cancel)?;
        }
    }
    fn drain(&mut self, cancel: &CancellationToken) -> Result<Vec<Value>> {
        loop {
            match self.incoming.try_recv() {
                Ok(Ok(message)) => self.accept(message, cancel)?,
                Ok(Err(error)) => {
                    self.closed = Some(error);
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.closed
                        .get_or_insert_with(|| "adapter closed its output".into());
                    break;
                }
            }
        }
        if self.events.is_empty() {
            if let Some(error) = &self.closed {
                return Err(fail(error.clone()));
            }
        }
        Ok(self.events.drain(..).collect())
    }
}

fn lock_slot<'a>(
    slot: &'a Mutex<Option<DebugSession>>,
    cancel: &CancellationToken,
) -> Result<MutexGuard<'a, Option<DebugSession>>> {
    loop {
        if cancel.is_cancelled() {
            return Err(fail("cancelled"));
        }
        match slot.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(20)),
            Err(TryLockError::Poisoned(_)) => return Err(fail("session lock poisoned")),
        }
    }
}

pub(crate) fn clear_session(owner: &str) {
    if let Ok(mut sessions) = sessions().lock() {
        sessions.retain(|(id, _), _| id != owner);
    }
}
pub(crate) fn shutdown_all() {
    if let Ok(mut sessions) = sessions().lock() {
        sessions.clear();
    }
}

pub struct DebugTool;
impl Tool for DebugTool {
    fn name(&self) -> &str {
        "debug"
    }
    fn description(&self) -> &str {
        "Debug through a DAP adapter. start launches an adapter and debuggee with optional source breakpoints; request sends DAP commands such as threads, stackTrace, scopes, variables, continue, next, stepIn, stepOut or evaluate; events drains adapter events; stop disconnects and kills the owned process tree. Sessions are private to this agent session. Starting, controlling and evaluating require approval."
    }
    fn parameters_schema(&self) -> Value {
        json!({"type":"object","properties":{
            "action":{"type":"string","enum":["start","request","events","list","stop"]},
            "session":{"type":"string","description":"Session name chosen by the model"},
            "adapter":{"type":"string","description":"DAP adapter executable for start"},
            "adapter_args":{"type":"array","items":{"type":"string"}},
            "launch":{"type":"object","description":"Adapter-specific launch arguments; program paths stay within the workspace"},
            "breakpoints":{"type":"array","items":{"type":"object","properties":{"path":{"type":"string"},"lines":{"type":"array","items":{"type":"integer"}}},"required":["path","lines"]}},
            "command":{"type":"string","description":"DAP request command"},
            "arguments":{"type":"object","description":"DAP request arguments"}
        },"required":["action"]})
    }
    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action")?;
        if action == "list" {
            let guard = sessions().lock().map_err(|_| fail("registry poisoned"))?;
            let mut names: Vec<_> = guard
                .keys()
                .filter(|(owner, _)| owner == &ctx.session_id)
                .map(|(_, name)| name.clone())
                .collect();
            names.sort();
            return Ok(json!({"sessions": names}).to_string());
        }
        let name = arg_str(args, "session")?;
        if name.trim().is_empty() {
            return Err(fail("session name is empty"));
        }
        let key = (ctx.session_id.clone(), name.clone());
        if action == "start" {
            let adapter = arg_str(args, "adapter")?;
            let adapter_args: Vec<String> = serde_json::from_value(
                args.get("adapter_args")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
            )
            .map_err(|_| fail("adapter_args must be strings"))?;
            let mut launch = args.get("launch").cloned().unwrap_or_else(|| json!({}));
            let launch_obj = launch
                .as_object_mut()
                .ok_or_else(|| fail("launch must be an object"))?;
            if let Some(program) = launch_obj.get("program").and_then(Value::as_str) {
                let path = super::sandbox::resolve_in_workspace(&ctx.cwd, program)?;
                launch_obj.insert("program".into(), json!(path));
            }
            if let Some(cwd) = launch_obj.get("cwd").and_then(Value::as_str) {
                let path = super::sandbox::resolve_in_workspace(&ctx.cwd, cwd)?;
                launch_obj.insert("cwd".into(), json!(path));
            } else {
                launch_obj.insert("cwd".into(), json!(ctx.cwd));
            }
            let breakpoints = match args.get("breakpoints") {
                Some(value) => value
                    .as_array()
                    .ok_or_else(|| fail("breakpoints must be an array"))?
                    .clone(),
                None => Vec::new(),
            };
            // Validate every path and line before the adapter or debuggee starts.
            let mut validated = Vec::new();
            for bp in breakpoints {
                let path = bp
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| fail("breakpoint path required"))?;
                let path = super::sandbox::resolve_in_workspace(&ctx.cwd, path)?;
                let lines = bp
                    .get("lines")
                    .and_then(Value::as_array)
                    .ok_or_else(|| fail("breakpoint lines required"))?;
                let mut points = Vec::new();
                for line in lines {
                    let line = line
                        .as_u64()
                        .filter(|line| *line > 0)
                        .ok_or_else(|| fail("breakpoint lines must be positive"))?;
                    points.push(json!({"line":line}));
                }
                validated.push((path, points));
            }
            let slot = Arc::new(Mutex::new(None));
            {
                let mut registry = sessions().lock().map_err(|_| fail("registry poisoned"))?;
                if registry.contains_key(&key) {
                    return Err(fail("session name already exists"));
                }
                registry.insert(key.clone(), slot.clone());
            }
            let result = (|| -> Result<Value> {
                let mut session = DebugSession::spawn(&adapter, &adapter_args, &ctx.cwd)?;
                let init = session.request("initialize", json!({"clientID":"nur","adapterID":"nur","linesStartAt1":true,"columnsStartAt1":true,"supportsRunInTerminalRequest":false}), &ctx.cancel)?;
                let capabilities = session.response(init, TIMEOUT, &ctx.cancel)?;
                let launch_id = session.request("launch", launch, &ctx.cancel)?;
                session.wait_initialized(launch_id, &ctx.cancel)?;
                let mut verified = Vec::new();
                for (path, points) in validated {
                    let req = session.request(
                        "setBreakpoints",
                        json!({"source":{"path":path},"breakpoints":points}),
                        &ctx.cancel,
                    )?;
                    verified.push(session.response(req, TIMEOUT, &ctx.cancel)?);
                }
                if capabilities
                    .get("supportsConfigurationDoneRequest")
                    .and_then(Value::as_bool)
                    == Some(true)
                {
                    let req = session.request("configurationDone", json!({}), &ctx.cancel)?;
                    session.response(req, TIMEOUT, &ctx.cancel)?;
                }
                session.response(launch_id, TIMEOUT, &ctx.cancel)?;
                *lock_slot(&slot, &ctx.cancel)? = Some(session);
                Ok(json!({"session":name,"breakpoints":verified}))
            })();
            if result.is_err() {
                if let Ok(mut registry) = sessions().lock() {
                    registry.remove(&key);
                }
            }
            return result.map(|value| value.to_string());
        }
        let slot = sessions()
            .lock()
            .map_err(|_| fail("registry poisoned"))?
            .get(&key)
            .cloned()
            .ok_or_else(|| fail("unknown session"))?;
        let mut guard = lock_slot(&slot, &ctx.cancel)?;
        let session = guard.as_mut().ok_or_else(|| fail("session is starting"))?;
        match action.as_str() {
            "request" => {
                let command = arg_str(args, "command")?;
                if ["initialize", "launch", "attach", "disconnect", "terminate"]
                    .contains(&command.as_str())
                {
                    return Err(fail("use start or stop for lifecycle operations"));
                }
                let arguments = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
                if !arguments.is_object() {
                    return Err(fail("arguments must be an object"));
                }
                let result = session
                    .request(&command, arguments, &ctx.cancel)
                    .and_then(|req| session.response(req, TIMEOUT, &ctx.cancel));
                if ctx.cancel.is_cancelled() {
                    guard.take();
                    if let Ok(mut registry) = sessions().lock() {
                        registry.remove(&key);
                    }
                }
                result.map(|value| value.to_string())
            }
            "events" => {
                let result = session.drain(&ctx.cancel);
                if ctx.cancel.is_cancelled() {
                    guard.take();
                    if let Ok(mut registry) = sessions().lock() {
                        registry.remove(&key);
                    }
                }
                result.map(|events| json!({"events":events}).to_string())
            }
            "stop" => {
                if let Ok(req) =
                    session.request("disconnect", json!({"terminateDebuggee":true}), &ctx.cancel)
                {
                    let _ = session.response(req, Duration::from_secs(3), &ctx.cancel);
                }
                guard.take();
                drop(guard);
                if let Ok(mut registry) = sessions().lock() {
                    registry.remove(&key);
                }
                Ok(format!("debug session {name} stopped"))
            }
            _ => Err(fail("unknown action")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::read_message;
    use std::io::Cursor;

    #[test]
    fn frame_accepts_utf8_byte_length_and_rejects_duplicate_lengths() {
        let body = "{\"text\":\"é\"}";
        let frame = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        assert_eq!(read_message(&mut Cursor::new(frame)).unwrap()["text"], "é");
        let duplicate = b"Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}";
        assert!(read_message(&mut Cursor::new(duplicate)).is_err());
    }

    #[test]
    fn frame_rejects_oversized_and_truncated_messages() {
        let huge = b"Content-Length: 999999999\r\n\r\n";
        assert!(read_message(&mut Cursor::new(huge)).is_err());
        let short = b"Content-Length: 10\r\n\r\n{}";
        assert!(read_message(&mut Cursor::new(short)).is_err());
    }
}
