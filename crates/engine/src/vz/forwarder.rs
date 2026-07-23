//! The port-forwarding runtime.
//!
//! [`super::forward`] decides *what* should be forwarded; this opens the host
//! listeners and keeps them in step with the running containers.

use crate::vz::forward::{bind_failure, diff, Forward};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;

/// Live listeners, keyed by the mapping that created them.
#[derive(Default)]
pub struct Forwarder {
    active: Mutex<BTreeMap<Forward, JoinHandle<()>>>,
}

impl Forwarder {
    pub fn new() -> Self {
        Self::default()
    }

    /// The mappings currently being served.
    pub fn active(&self) -> std::collections::BTreeSet<Forward> {
        self.active.lock().unwrap().keys().cloned().collect()
    }

    /// Stop everything. Called when the engine goes down, so host ports are
    /// released rather than held by a forwarder pointing at a dead guest.
    pub fn shutdown(&self) {
        for (_, handle) in std::mem::take(&mut *self.active.lock().unwrap()) {
            handle.abort();
        }
    }

    fn insert(&self, forward: Forward, handle: JoinHandle<()>) {
        self.active.lock().unwrap().insert(forward, handle);
    }

    fn remove(&self, forward: &Forward) {
        if let Some(handle) = self.active.lock().unwrap().remove(forward) {
            handle.abort();
        }
    }
}

impl Drop for Forwarder {
    fn drop(&mut self) {
        for (_, handle) in std::mem::take(self.active.get_mut().unwrap()) {
            handle.abort();
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use crate::vz::forward::AGENT_VSOCK_PORT;
    use crate::vz::machine::Machine;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    impl Forwarder {
        /// Bring the live listeners in line with `wanted`.
        ///
        /// Returns any mapping that could not be bound, with a reason, so the
        /// UI can tell the user *why* their port is not reachable instead of
        /// leaving them to discover it with curl.
        pub async fn sync(
            &self,
            machine: &Arc<Machine>,
            wanted: std::collections::BTreeSet<Forward>,
        ) -> Vec<(Forward, String)> {
            let (start, stop) = diff(&self.active(), &wanted);

            for forward in stop {
                tracing::info!("releasing forwarded port {}", forward.host_port);
                self.remove(&forward);
            }

            let mut failures = Vec::new();
            for forward in start {
                match TcpListener::bind(forward.bind_addr()).await {
                    Ok(listener) => {
                        tracing::info!(
                            "forwarding {}:{} to guest {}",
                            forward.host_ip,
                            forward.host_port,
                            forward.guest_port
                        );
                        let handle =
                            tokio::spawn(accept_loop(listener, Arc::clone(machine), forward.clone()));
                        self.insert(forward, handle);
                    }
                    Err(e) => failures.push((forward.clone(), bind_failure(&forward, &e))),
                }
            }
            failures
        }
    }

    /// Serve one forwarded port until the task is aborted.
    async fn accept_loop(listener: TcpListener, machine: Arc<Machine>, forward: Forward) {
        loop {
            let Ok((mut host_side, _)) = listener.accept().await else {
                continue;
            };
            let machine = Arc::clone(&machine);
            let request = forward.agent_request();
            // One task per connection so a long-lived stream does not block
            // the next request to the same port.
            tokio::spawn(async move {
                let mut guest_side = match machine.connect(AGENT_VSOCK_PORT).await {
                    Ok(stream) => stream,
                    Err(e) => {
                        tracing::warn!("port forward could not reach the guest agent: {e}");
                        return;
                    }
                };
                // The agent reads one line naming its target, then splices.
                if guest_side.write_all(request.as_bytes()).await.is_err() {
                    return;
                }
                let _ = tokio::io::copy_bidirectional(&mut host_side, &mut guest_side).await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn forward(host_port: u16) -> Forward {
        Forward {
            host_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            host_port,
            guest_port: 80,
            proto: "tcp".into(),
        }
    }

    #[tokio::test]
    async fn a_new_forwarder_serves_nothing() {
        assert!(Forwarder::new().active().is_empty());
    }

    #[tokio::test]
    async fn tracked_mappings_are_reported_and_released() {
        let f = Forwarder::new();
        let handle = tokio::spawn(async { std::future::pending::<()>().await });
        f.insert(forward(18080), handle);
        assert_eq!(f.active().len(), 1);

        f.remove(&forward(18080));
        assert!(
            f.active().is_empty(),
            "a released mapping must free its host port"
        );
    }

    #[tokio::test]
    async fn shutdown_releases_every_port() {
        let f = Forwarder::new();
        for port in [18081, 18082, 18083] {
            let handle = tokio::spawn(async { std::future::pending::<()>().await });
            f.insert(forward(port), handle);
        }
        assert_eq!(f.active().len(), 3);

        f.shutdown();
        assert!(
            f.active().is_empty(),
            "an engine going down must not leave ports held by a dead forwarder"
        );
    }

    #[tokio::test]
    async fn aborted_tasks_actually_stop() {
        let f = Forwarder::new();
        let handle = tokio::spawn(async { std::future::pending::<()>().await });
        let abort = handle.abort_handle();
        f.insert(forward(18084), handle);
        f.shutdown();
        // Give the runtime a moment to process the abort.
        tokio::task::yield_now().await;
        assert!(abort.is_finished());
    }
}
