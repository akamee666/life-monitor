use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};
use tracing::error;

/// Spawns a new asynchronous task that sends a message on a channel at a regular interval.
pub fn spawn_ticker<T>(tx: mpsc::Sender<T>, duration: Duration, event_to_send: T) -> JoinHandle<()>
where
    T: Clone + Send + 'static,
{
    tokio::spawn(async move {
        let mut interval = interval(duration);
        interval.tick().await;
        loop {
            interval.tick().await;
            if tx.send(event_to_send.clone()).await.is_err() {
                error!("Ticker channel closed. Shutting down ticker task");
                break;
            }
        }
    })
}
