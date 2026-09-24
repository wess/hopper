use std::time::Duration;

use docker::{Client, Endpoint};
use host::Host;

#[tokio::test]
async fn event_stream_waits_for_provider_selection() {
    let host = Host::new(Client::new(Endpoint::Unix {
        path: "/nonexistent-hopper-selection.sock".into(),
    }));

    assert!(!host.selection_ready());
    assert!(
        tokio::time::timeout(Duration::from_millis(50), host.wait_for_initial_selection())
            .await
            .is_err(),
        "an event stream must not attach to the construction-time endpoint"
    );
    assert!(!host.selection_ready());

    tokio::time::timeout(Duration::from_secs(20), host.select_engine())
        .await
        .expect("provider selection completes");
    tokio::time::timeout(Duration::from_secs(1), host.wait_for_initial_selection())
        .await
        .expect("event stream is released after selection");
    assert!(host.selection_ready());
}
