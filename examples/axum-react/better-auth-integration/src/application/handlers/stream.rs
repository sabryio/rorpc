use async_stream::stream;
use axum::{
    extract::State,
    response::sse::{Event, KeepAlive},
    response::Sse,
};
use std::{convert::Infallible, time::Duration};
use tokio_stream::{iter, Stream, StreamExt};

use crate::{
    domain::models::{events::SseEvent, planet::EventData},
    infrastructure::context::AppState,
};

// ---------------------------------------------------------------------------
// SSE helpers
// ---------------------------------------------------------------------------

fn sse_flush() -> Result<Event, Infallible> {
    Ok(Event::default().comment(""))
}

fn sse_close() -> Result<Event, Infallible> {
    Ok(Event::default().event("close").data(""))
}

fn sse_message<T: serde::Serialize>(id: impl ToString, payload: &T) -> Result<Event, Infallible> {
    let data = serde_json::to_string(payload).unwrap_or_default();
    Ok(Event::default()
        .event("message")
        .id(id.to_string())
        .retry(Duration::from_secs(5))
        .data(data))
}

/// Wraps an inner stream with a flush header and close trailer.
/// Caller builds the `Sse` response to allow customisation of keep-alive etc.
fn sse_stream<S>(inner: S) -> impl Stream<Item = Result<Event, Infallible>> + Send + 'static
where
    S: Stream<Item = Result<Event, Infallible>> + Send + 'static,
{
    iter([sse_flush()]).chain(inner).chain(iter([sse_close()]))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

#[rorpc::get("/stream", data = "EventData")]
pub async fn stream_events(
    State(_state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    Sse::new(sse_stream(
        iter(0u32..)
            .throttle(Duration::from_secs(1))
            .take(10)
            .map(|count| {
                sse_message(
                    count,
                    &EventData {
                        message: format!("Event #{count}"),
                        count,
                    },
                )
            }),
    ))
    .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text(""))
}

#[rorpc::get("/stream-async", data = "EventData")]
pub async fn stream_events_async(
    State(_state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    Sse::new(sse_stream(stream! {
        for i in 0u32..15 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            yield sse_message(i, &EventData {
                message: format!("Async Stream Event #{i}"),
                count: i,
            });
        }
    }))
    .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text(""))
}

/// Stream campaign events - demonstrates SseEvent enum with tagged union serialization
#[rorpc::get("/stream-campaigns", data = "SseEvent")]
pub async fn stream_campaign_events(
    State(_state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    Sse::new(sse_stream(stream! {
        // Simulate campaign lifecycle events

        // Event 1: Campaign created
        tokio::time::sleep(Duration::from_secs(1)).await;
        yield sse_message(
            0,
            &SseEvent::CampaignCreated {
                campaign_id: "campaign-001".to_string(),
                title: "Summer Sale Campaign".to_string(),
            },
        );

        // Event 2: Status changed
        tokio::time::sleep(Duration::from_secs(2)).await;
        yield sse_message(
            1,
            &SseEvent::CampaignStatusChanged {
                campaign_id: "campaign-001".to_string(),
                status: "running".to_string(),
            },
        );

        // Event 3-7: Progress updates
        for i in 0..5 {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            yield sse_message(
                2 + i,
                &SseEvent::CampaignProgress {
                    campaign_id: "campaign-001".to_string(),
                    sent: (i + 1) * 20,
                    total: 100,
                    failed: i,
                },
            );
        }

        // Event 8: Another campaign created
        tokio::time::sleep(Duration::from_secs(1)).await;
        yield sse_message(
            7,
            &SseEvent::CampaignCreated {
                campaign_id: "campaign-002".to_string(),
                title: "Holiday Promotion".to_string(),
            },
        );

        // Event 9: Final status
        tokio::time::sleep(Duration::from_secs(2)).await;
        yield sse_message(
            8,
            &SseEvent::CampaignStatusChanged {
                campaign_id: "campaign-001".to_string(),
                status: "completed".to_string(),
            },
        );
    }))
    .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text(""))
}
