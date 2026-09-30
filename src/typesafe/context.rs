//! Cancellation and ownership follow judgments across async/blocking workers.
use std::cell::RefCell;
use std::future::Future;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct Scope {
    pub cancel: CancellationToken,
    pub session_id: String,
    pub turn_id: String,
    pub deadline: Option<Instant>,
}
tokio::task_local! { static TASK: Scope; }
thread_local! { static THREAD: RefCell<Option<Scope>> = const { RefCell::new(None) }; }

pub fn current() -> Option<Scope> {
    THREAD
        .with(|s| s.borrow().clone())
        .or_else(|| TASK.try_with(Clone::clone).ok())
}

pub async fn scope<T>(
    session_id: String,
    cancel: CancellationToken,
    future: impl Future<Output = T>,
) -> T {
    TASK.scope(
        Scope {
            session_id,
            turn_id: uuid::Uuid::new_v4().to_string(),
            cancel,
            deadline: None,
        },
        future,
    )
    .await
}

pub fn with_sync<T>(scope: Option<Scope>, operation: impl FnOnce() -> T) -> T {
    struct Restore(Option<Scope>);
    impl Drop for Restore {
        fn drop(&mut self) {
            THREAD.with(|s| *s.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(THREAD.with(|s| s.replace(scope)));
    operation()
}

pub fn request<T>(timeout: Duration, operation: impl FnOnce() -> T) -> T {
    let mut scope = current().unwrap_or_else(|| Scope {
        cancel: CancellationToken::new(),
        session_id: String::new(),
        turn_id: String::new(),
        deadline: None,
    });
    let deadline = Instant::now() + timeout;
    scope.deadline = Some(scope.deadline.map_or(deadline, |old| old.min(deadline)));
    with_sync(Some(scope), operation)
}

pub fn check() -> Result<(), String> {
    if let Some(scope) = current() {
        if scope.cancel.is_cancelled() {
            return Err("judgment cancelled".into());
        }
        if scope.deadline.is_some_and(|d| Instant::now() >= d) {
            return Err("judgment deadline exceeded".into());
        }
    }
    Ok(())
}

pub fn spawn_blocking<T: Send + 'static>(
    operation: impl FnOnce() -> T + Send + 'static,
) -> tokio::task::JoinHandle<T> {
    let scope = current();
    tokio::task::spawn_blocking(move || with_sync(scope, operation))
}

pub async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let scope = current();
    let cancel = scope.as_ref().map(|s| s.cancel.clone()).unwrap_or_default();
    let task = tokio::task::spawn_blocking(move || with_sync(scope, operation));
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err("judgment cancelled".into()),
        result = task => result.map_err(|e| e.to_string()),
    }
}
