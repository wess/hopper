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
            // Work here is asynchronous I/O; archive production uses the
            // blocking pool. Do not reserve one worker per host CPU.
            .worker_threads(2)
            .thread_name("hopper-io")
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
    run_owned(cx, fut, done).detach();
}

pub fn run_owned<T: Send + 'static>(
    cx: &mut App,
    fut: impl Future<Output = T> + Send + 'static,
    done: impl FnOnce(T, &mut App) + 'static,
) -> gpui::Task<()> {
    let (tx, rx) = futures::channel::oneshot::channel();
    let task = AbortOnDrop(runtime().spawn(async move {
        let _ = tx.send(fut.await);
    }));
    cx.spawn(async move |cx| {
        let _task = task;
        if let Ok(result) = rx.await {
            let _ = cx.update(|cx| done(result, cx));
        }
    })
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
    on_item: impl FnMut(T, &mut App) + 'static,
    on_done: impl FnOnce(&mut App) + 'static,
) where
    T: Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    stream_owned(cx, producer, on_item, on_done).detach();
}

/// A view owns the returned task. Dropping it cancels both sides immediately,
/// including a producer blocked waiting for a silent daemon.
pub fn stream_owned<T, Fut>(
    cx: &mut App,
    producer: impl FnOnce(mpsc::Sender<T>) -> Fut + Send + 'static,
    mut on_item: impl FnMut(T, &mut App) + 'static,
    on_done: impl FnOnce(&mut App) + 'static,
) -> gpui::Task<()>
where
    T: Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let (tx, mut rx) = mpsc::channel(512);
    // Construct the guard before spawning the consumer so cancellation also
    // works if the consumer has never been polled.
    let task = AbortOnDrop(runtime().spawn(producer(tx)));
    cx.spawn(async move |cx| {
        let _task = task;
        while let Some(item) = rx.next().await {
            if cx.update(|cx| on_item(item, cx)).is_err() {
                return;
            }
        }
        let _ = cx.update(|cx| on_done(cx));
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn cancelling_an_owner_drops_a_silent_producer_without_waiting_for_output() {
        let (ready_tx, ready) = tokio::sync::oneshot::channel();
        let (closed_tx, closed) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _closed = closed_tx;
            ready_tx.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        let owner = abort_on_drop(task);
        ready.await.unwrap();
        drop(owner);
        assert!(tokio::time::timeout(Duration::from_secs(1), closed)
            .await
            .unwrap()
            .is_err());
    }

    #[tokio::test]
    async fn cancellation_also_drops_an_unpolled_producer() {
        let (sender, receiver) = tokio::sync::oneshot::channel::<()>();
        let owner = abort_on_drop(tokio::spawn(async move {
            let _sender = sender;
            std::future::pending::<()>().await;
        }));
        drop(owner);
        assert!(tokio::time::timeout(Duration::from_secs(1), receiver)
            .await
            .unwrap()
            .is_err());
    }
}
