use async_fetcher_preview::*;
use futures03::{channel::mpsc, prelude::*};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

pub async fn fetch<F>(
    path: Arc<Path>,
    url: Box<str>,
    iso_size: u64,
    progress: Arc<F>,
    cancel: &Arc<AtomicBool>,
) -> Result<(), async_fetcher_preview::Error>
where
    F: Fn(u64, u64) + 'static + Send + Sync,
{
    // A channel for setting up an event-handler.
    let (etx, mut erx) = mpsc::unbounded();

    // The fetcher's event-handler, which will handle all progress events.
    let events = async move {
        let mut total = 0;
        while let Some((_, event)) = erx.next().await {
            if let FetchEvent::Progress(written) = event {
                total += written as u64;

                (*progress)(total / 1024, iso_size / 1024);
            }
        }
    };

    // The future which carries out the ISO-fetching task.
    let request = Fetcher::new(surf::Client::new())
        // Allow the daemon to cancel the fetching process at any time
        .cancel(cancel.clone())
        // Use 4 connections to fetch 4 parts concurrently
        .connections_per_file(4)
        // Fetch in 4 MiB chunks per connection
        .max_part_size(4 * 1024 * 1024)
        // Send all callback events to our `erx` receiver
        .events(etx)
        // Time out if a chunk fails to make progress within 15 seconds.
        .timeout(Duration::from_secs(15))
        // Wrap our fetcher in an Arc — required for our API.
        .into_arc()
        // Submit the fetch request
        .request(vec![url].into(), path.clone());

    // Concurrently await on our event-handler and fetcher to complete.
    let (_, res) = futures03::future::join(events, request).await;

    cancel.store(false, Ordering::SeqCst);

    res.map(|_| ())
}
