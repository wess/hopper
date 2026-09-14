//! The tokio ↔ gpui bridge.
//!
//! The Docker layer runs on a tokio runtime; gpui has its own executor. Views
//! dispatch an async `Host` call here: it runs on the shared tokio runtime and
//! the result is delivered on the gpui main thread through a runtime-agnostic
//! oneshot. This is the single seam every Docker dispatch goes through.

use std::future::Future;
use std::sync::OnceLock;

use futures::channel::mpsc;
use futures::StreamExt;
use gpui::App;
use tokio::runtime::Runtime;

/// A Tokio task must be explicitly aborted when the gpui-side future is
/// dropped. Dropping a Tokio `JoinHandle` detaches the task; that would leave
/// a cancelled view's Docker request running until its normal timeout.
pub(crate) struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Keep a Tokio task tied to the lifetime of its gpui-side future.
pub(crate) fn abort_on_drop(task: tokio::task::JoinHandle<()>) -> AbortOnDrop {
    AbortOnDrop(task)
}

/// The process-wide tokio runtime, created on first use.
pub fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime")
    })
}

/// Run `fut` on the tokio runtime, then `done` on the gpui main thread.
pub fn run<T: Send + 'static>(
    cx: &mut App,
    fut: impl Future<Output = T> + Send + 'static,
    done: impl FnOnce(T, &mut App) + 'static,
) {
    let (tx, rx) = futures::channel::oneshot::channel();
    let task = runtime().spawn(async move {
        let _ = tx.send(fut.await);
    });
    cx.spawn(async move |cx| {
        let _task = AbortOnDrop(task);
        if let Ok(result) = rx.await {
            let _ = cx.update(|cx| done(result, cx));
        }
    })
    .detach();
}

/// Run a streaming producer on the tokio runtime, delivering each item to
/// `on_item` on the gpui main thread as it arrives, then `on_done` when the
/// producer finishes.
///
/// The producer is handed a bounded sender. When the receiving side goes away
/// or the queue is full, the sender's `try_send` fails; that is how a closed or
/// overloaded view cancels a stream instead of growing memory without limit.
pub fn stream<T, Fut>(
    cx: &mut App,
    producer: impl FnOnce(mpsc::Sender<T>) -> Fut + Send + 'static,
    mut on_item: impl FnMut(T, &mut App) + 'static,
    on_done: impl FnOnce(&mut App) + 'static,
) where
    T: Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let (tx, mut rx) = mpsc::channel(512);
    let task = runtime().spawn(producer(tx));
    cx.spawn(async move |cx| {
        let _task = AbortOnDrop(task);
        while let Some(item) = rx.next().await {
            if cx.update(|cx| on_item(item, cx)).is_err() {
                return;
            }
        }
        let _ = cx.update(|cx| on_done(cx));
    })
    .detach();
}
