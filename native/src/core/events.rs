//! The one Event API cursor (Syncshell `internal/session/events.go`): a
//! filtered long-poll (1 s) that re-hydrates on configuration and state
//! events or a cursor gap, feeds the activity rotation, and runs the
//! scheduled full refresh, lifecycle probe and activity rotation. Errors
//! mark the snapshot stale and back off (250 ms doubling to 8 s, plus a
//! fixed jitter) without delaying a scheduled refresh or probe.

use std::thread;
use std::time::{Duration, Instant};

use super::activity::ACTIVITY_CYCLE;
use super::client::Event;
use super::session::Session;
use super::types::Published;

const EVENT_POLL_SECONDS: u64 = 1;

pub const EVENT_TYPES: [&str; 18] = [
    "ConfigSaved",
    "DeviceConnected",
    "DeviceDisconnected",
    "DownloadProgress",
    "FolderErrors",
    "FolderPaused",
    "FolderResumed",
    "FolderScanProgress",
    "FolderSummary",
    "ItemFinished",
    "ItemStarted",
    "LocalIndexUpdated",
    "PendingDevicesChanged",
    "PendingFoldersChanged",
    "DeviceDiscovered",
    "RemoteDownloadProgress",
    "StateChanged",
    "StartupComplete",
];

/// Runs forever, handing every changed snapshot to `emit`.
pub fn run(session: &Session, emit: &dyn Fn(Published)) {
    let (mut cursor, mut retry) = match session.client.events(0, 1, 0, &EVENT_TYPES) {
        Ok(events) => (events.last().map_or(0, |e| e.id), 0u32),
        Err(_) => {
            emit_latest(emit, session.set_fresh(false));
            (0, 1)
        }
    };
    let mut schedule = Schedule {
        refresh: Instant::now() + session.refresh_interval(),
        lifecycle: Instant::now() + session.probe_interval(),
        activity: Instant::now() + ACTIVITY_CYCLE,
    };
    loop {
        let (since, limit) = if retry > 0 { (0, 1) } else { (cursor, 256) };
        let events = match session
            .client
            .events(since, limit, EVENT_POLL_SECONDS, &EVENT_TYPES)
        {
            Ok(events) => events,
            Err(_) => {
                emit_latest(emit, session.set_fresh(false));
                let now = Instant::now();
                let mut delay = retry_delay(retry);
                for deadline in [schedule.refresh, schedule.lifecycle, schedule.activity] {
                    delay = delay.min(deadline.saturating_duration_since(now));
                }
                thread::sleep(delay);
                retry += 1;
                schedule.reconcile(session, emit, Instant::now());
                continue;
            }
        };
        if retry > 0 {
            // Keep unread activity when the old sequence survived a restart.
            if events.last().is_none_or(|e| e.id < cursor) {
                cursor = 0;
            }
            emit_latest(emit, session.refresh().0);
            schedule.refreshed(session, Instant::now());
            retry = 0;
            continue;
        }
        if !events.is_empty() {
            let gap = discontinuity(cursor, &events);
            cursor = events.last().map_or(cursor, |e| e.id);
            if gap || requires_hydration(&events) {
                emit_latest(emit, session.refresh().0);
                schedule.refreshed(session, Instant::now());
            }
            if session.process_activity_events(&events, Instant::now()) {
                emit_latest(emit, session.publish_activity(false, Instant::now()));
            }
        }
        schedule.reconcile(session, emit, Instant::now());
    }
}

struct Schedule {
    refresh: Instant,
    lifecycle: Instant,
    activity: Instant,
}

impl Schedule {
    fn refreshed(&mut self, session: &Session, now: Instant) {
        self.refresh = now + session.refresh_interval();
        self.lifecycle = now + session.probe_interval();
    }

    fn reconcile(&mut self, session: &Session, emit: &dyn Fn(Published), now: Instant) {
        if session.take_config_changed() {
            self.refresh = now;
            self.lifecycle = now;
        }
        if now >= self.activity {
            emit_latest(emit, session.publish_activity(true, now));
            self.activity = now + ACTIVITY_CYCLE;
        }
        if now >= self.refresh {
            emit_latest(emit, session.refresh().0);
            self.refreshed(session, now);
        } else if now >= self.lifecycle {
            emit_latest(emit, session.refresh_lifecycle());
            self.lifecycle = now + session.probe_interval();
        }
    }
}

fn emit_latest(emit: &dyn Fn(Published), published: Published) {
    if published.revision != 0 {
        emit(published);
    }
}

/// True when the IDs after `cursor` are not consecutive (Syncthing dropped
/// events or restarted).
pub fn discontinuity(cursor: i64, events: &[Event]) -> bool {
    let mut expected = cursor + 1;
    for (index, event) in events.iter().enumerate() {
        if (cursor != 0 || index > 0) && event.id != expected {
            return true;
        }
        expected = event.id + 1;
    }
    false
}

/// Everything but transfer progress changes the hydrated state.
pub fn requires_hydration(events: &[Event]) -> bool {
    events.iter().any(|e| {
        !matches!(
            e.kind.as_str(),
            "RemoteDownloadProgress" | "DownloadProgress" | "ItemStarted" | "ItemFinished"
        )
    })
}

/// 250 ms × 2^attempt (attempt ≤ 5) plus a fixed 0–100 ms jitter.
pub fn retry_delay(attempt: u32) -> Duration {
    let attempt = attempt.min(5);
    Duration::from_millis(250 * (1 << attempt) + u64::from((attempt * 97 + 53) % 101))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(ids: &[i64]) -> Vec<Event> {
        ids.iter()
            .map(|&id| Event {
                id,
                kind: "ItemStarted".into(),
                data: None,
            })
            .collect()
    }

    #[test]
    fn gaps_are_detected_after_the_cursor() {
        assert!(!discontinuity(0, &events(&[7, 8, 9])));
        assert!(!discontinuity(6, &events(&[7, 8])));
        assert!(discontinuity(5, &events(&[7])));
        assert!(discontinuity(0, &events(&[7, 9])));
    }

    #[test]
    fn retry_delays_are_bounded() {
        assert_eq!(retry_delay(0), Duration::from_millis(303));
        assert_eq!(retry_delay(1), Duration::from_millis(500 + 49));
        assert_eq!(retry_delay(5), retry_delay(9));
        assert_eq!(retry_delay(5), Duration::from_millis(8000 + 33));
    }

    #[test]
    fn progress_alone_needs_no_hydration() {
        assert!(!requires_hydration(&events(&[1, 2])));
        let mut mixed = events(&[1]);
        mixed.push(Event {
            id: 2,
            kind: "StateChanged".into(),
            data: None,
        });
        assert!(requires_hydration(&mixed));
    }
}
