//! Read monitor state without adding model input or blocking terminal input.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadBackgroundTerminalsListParams;
use codex_app_server_protocol::ThreadBackgroundTerminalsListResponse;
use codex_app_server_protocol::ThreadMonitor;
use codex_protocol::ThreadId;

pub(super) type MonitorStatusRequest =
    Pin<Box<dyn Future<Output = Option<Vec<ThreadMonitor>>> + Send>>;

pub(super) fn request_monitor_status(
    handle: AppServerRequestHandle,
    thread_id: ThreadId,
) -> MonitorStatusRequest {
    Box::pin(async move {
        tokio::time::timeout(
            Duration::from_secs(5),
            handle.request_typed::<ThreadBackgroundTerminalsListResponse>(
                ClientRequest::ThreadBackgroundTerminalsList {
                    request_id: RequestId::String(format!(
                        "monitor-status-{}",
                        uuid::Uuid::new_v4()
                    )),
                    params: ThreadBackgroundTerminalsListParams {
                        thread_id: thread_id.to_string(),
                        cursor: None,
                        limit: Some(1),
                    },
                },
            ),
        )
        .await
        .ok()
        .and_then(Result::ok)
        .map(|response| response.monitors)
    })
}
