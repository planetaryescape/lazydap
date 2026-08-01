//! `--wait`: turning a push-based adapter into one blocking call.
//!
//! A DAP adapter tells you what happened whenever it feels like it. A shell
//! agent runs one command and expects one answer. This module is the bridge:
//! send an execution request, collect everything that arrives until the
//! program reaches a stable state, and hand back a single [`StableState`].
//!
//! The design and every edge case are in `docs/blueprint/10-async-to-sync.md`.
//! Three things here are load-bearing and easy to get subtly wrong:
//!
//! 1. **Subscribe before sending.** An event can arrive before the request's
//!    own acknowledgement does. [`Wait::begin`] takes the subscription, so a
//!    caller cannot send first by accident — the type does not let you.
//! 2. **The buffer and the subscription overlap.** Output produced between two
//!    CLI invocations is buffered, not broadcast to us. Both are read, and the
//!    sequence watermark is what stops an event landing in both.
//! 3. **A late `exited` still counts.** DAP does not order `exited` before
//!    `terminated`, so a blob emitted the instant the session ends can be
//!    missing the exit code the caller rang up to get.

use crate::state::{SeqEvent, Session};
use lazydap_core::{EndReason, WaitOutcome};
use lazydap_protocol::{Event, StableState};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::Instant;

/// The default a caller gets by saying nothing. Long enough for normal
/// debugging, short enough that a wedged session does not lock up an agent.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to keep listening after the first `stopped`, for the other
/// threads of a multi-threaded program (D020).
const COALESCE_WINDOW: Duration = Duration::from_millis(50);

/// How long to keep looking for an exit code after the session ends.
///
/// The same window the launch handshake uses, for the same reason: `exited`
/// and `terminated` are not ordered, and the blob is the only thing the caller
/// reads.
const EXIT_CODE_GRACE: Duration = Duration::from_millis(250);
/// How often to look during that window.
const EXIT_CODE_POLL: Duration = Duration::from_millis(10);

/// Most output a single wait will carry, before it starts dropping it (D9).
const OUTPUT_CAP_BYTES: usize = 1_000_000;

pub struct WaitOptions {
    /// `None` waits forever, which the caller has asked for explicitly.
    pub timeout: Option<Duration>,
    /// Wait for every thread to stop rather than returning on the first.
    pub all_threads: bool,
}

/// A wait that has already subscribed and is safe to send a request behind.
///
/// Begin it, send the DAP request, then collect. Doing it in that order is not
/// a convention — `begin` is the only way to make one of these, and it
/// subscribes on the way.
pub struct Wait {
    session: Arc<Session>,
    events: tokio::sync::broadcast::Receiver<SeqEvent>,
    /// Live events at or below this were already taken from the buffer.
    watermark: u64,
    started: Instant,
    blob: StableState,
}

impl Wait {
    /// Subscribe, and read everything buffered that no earlier wait reported.
    ///
    /// Reading, not consuming: nothing is marked delivered until
    /// [`collect`](Self::collect) actually returns a blob. A wait whose
    /// request is rejected reports nothing, and its backlog has to still be
    /// there for the next one.
    pub fn begin(session: &Arc<Session>) -> Self {
        // Subscription first. An event that arrives between these two lines is
        // seen twice — once here, once live — and the watermark is what
        // resolves that. An event arriving between them the other way round
        // would simply be lost.
        let events = session.subscribe();
        let (pending, watermark, lost) = session.undelivered();

        let mut wait = Self {
            session: Arc::clone(session),
            events,
            watermark,
            started: Instant::now(),
            blob: StableState::new(WaitOutcome::Timeout),
        };
        // Events that fell out of the session buffer before this wait could
        // read them. A debuggee chatty enough to overrun the buffer between two
        // CLI invocations loses the *beginning* of its own output, and a blob
        // that reported the survivors as the whole story would be handing back
        // a suffix while claiming nothing was missing (D072).
        wait.record_loss(lost);
        for event in pending {
            wait.absorb_backlog(&event);
        }
        wait
    }

    /// Record that `count` events are missing from this blob, whatever the
    /// reason. `output_truncated` is what a reader checks, so every cause has
    /// to reach it.
    fn record_loss(&mut self, count: u64) {
        if count == 0 {
            return;
        }
        self.blob.dropped_events += count;
        self.blob.output_truncated = true;
    }

    /// Block until the program settles, and describe what happened.
    pub async fn collect(mut self, options: WaitOptions) -> StableState {
        let deadline = options.timeout.map(|timeout| self.started + timeout);

        let outcome = self.run(deadline, options.all_threads).await;
        self.blob.state = outcome;

        if outcome == WaitOutcome::Paused {
            if !options.all_threads {
                self.coalesce().await;
            }
            self.fetch_top_frame().await;
        }

        if !outcome.is_live() && self.blob.exit_code.is_none() {
            self.grace_for_exit_code().await;
        }

        // Commit delivery here and nowhere else: this is the point at which a
        // blob is certainly being returned. That covers the backlog read at
        // `begin` as well as everything consumed live, and it means a wait
        // that never got this far has silently cost nothing.
        self.session.mark_delivered(self.watermark);

        self.blob.elapsed_ms = self.started.elapsed().as_millis() as u64;
        self.blob
    }

    /// Read events until one of them ends the wait.
    async fn run(&mut self, deadline: Option<Instant>, all_threads: bool) -> WaitOutcome {
        loop {
            let received = match deadline {
                Some(deadline) => match tokio::time::timeout_at(deadline, self.events.recv()).await
                {
                    Ok(received) => received,
                    // The program is still running. We do not pause it: an
                    // automatic pause on timeout would be a side effect nobody
                    // asked for, and can mask the very hang being diagnosed.
                    Err(_) => return WaitOutcome::Timeout,
                },
                None => self.events.recv().await,
            };

            let sequenced = match received {
                Ok(sequenced) => sequenced,
                // Slower than the daemon: some events are gone for good. Say
                // so rather than presenting a gap as the whole story.
                Err(RecvError::Lagged(missed)) => {
                    tracing::warn!(target: "daemon.session", missed, "a wait fell behind its events");
                    self.record_loss(missed);
                    continue;
                }
                // The session's sender is gone, which only happens once
                // everything holding it has been dropped.
                Err(RecvError::Closed) => return WaitOutcome::AdapterDied,
            };

            if sequenced.seq <= self.watermark
                || sequenced.event.session_id() != Some(self.session.id)
            {
                continue;
            }
            self.watermark = sequenced.seq;

            if let Some(outcome) = self.consider(&sequenced.event, all_threads) {
                return outcome;
            }
        }
    }

    /// Fold one event in, and say whether it ended the wait.
    fn consider(&mut self, event: &Event, all_threads: bool) -> Option<WaitOutcome> {
        self.absorb(event);

        match event {
            Event::Stopped {
                all_threads_stopped,
                ..
            } => {
                // `--all-threads` means "give me a complete cross-thread
                // snapshot", so a stop that only accounts for one thread is
                // not yet the answer.
                if all_threads && !all_threads_stopped {
                    return None;
                }
                Some(WaitOutcome::Paused)
            }
            Event::SessionEnded { reason, .. } => Some(match reason {
                EndReason::Exited { .. } => WaitOutcome::Exited,
                EndReason::AdapterDied { .. } => WaitOutcome::AdapterDied,
                EndReason::Disconnected | EndReason::Terminated => WaitOutcome::Terminated,
            }),
            _ => None,
        }
    }

    /// Carry forward what happened *before* this wait started.
    ///
    /// Only the things nobody has seen yet and that still describe the run:
    /// output the program printed, and breakpoint or thread changes. The stop
    /// the program is currently sitting on is deliberately not one of them —
    /// it is where the run began, not how it ended, and letting it name the
    /// blob would report every `continue` as having stopped for the reason the
    /// *previous* one did.
    fn absorb_backlog(&mut self, event: &Event) {
        match event {
            Event::Output { .. }
            | Event::BreakpointUpdated { .. }
            | Event::ThreadChanged { .. } => self.absorb(event),
            _ => {}
        }
    }

    /// Record what an event says, without judging whether it ends anything.
    fn absorb(&mut self, event: &Event) {
        match event {
            // Once the cap is hit the wait stops taking output, for good.
            //
            // Skipping only the chunk that would overrun it and going on
            // accepting the smaller ones behind it made `captured_output` a
            // *splice*: two hundred lines vanished from the middle of a run and
            // text produced most of a second later was concatenated straight
            // onto a mid-line cut, with nothing marking the join.
            // `output_truncated` is universally read as "the tail was cut", so
            // what it flagged was not what it meant. Stopping here makes the
            // retained output a strict prefix of what the program printed,
            // which is the only shape that claim is true of (D070).
            Event::Output { chunk, .. } if !self.blob.output_truncated => {
                let buffered: usize = self
                    .blob
                    .captured_output
                    .iter()
                    .map(|chunk| chunk.output.len())
                    .sum();
                if buffered + chunk.output.len() > OUTPUT_CAP_BYTES {
                    self.blob.output_truncated = true;
                } else {
                    self.blob.captured_output.push(chunk.clone());
                }
            }
            Event::Output { .. } => {}
            Event::Stopped {
                thread_id,
                adapter_thread_id,
                reason,
                raw_reason,
                all_threads_stopped,
                hit_breakpoint_ids,
                ..
            } => {
                // The first stop names the blob; later ones in the coalescing
                // window are additional threads, not a change of story.
                if self.blob.reason.is_none() {
                    self.blob.reason = Some(reason.clone());
                    self.blob.raw_reason = raw_reason.clone();
                    self.blob.thread_id = *thread_id;
                    self.blob.adapter_thread_id = *adapter_thread_id;
                    self.blob.all_threads_stopped = *all_threads_stopped;
                    self.blob.hit_breakpoint_ids = hit_breakpoint_ids.clone();
                } else if let Some(thread_id) = thread_id {
                    if Some(*thread_id) != self.blob.thread_id
                        && !self.blob.additional_stopped_threads.contains(thread_id)
                    {
                        self.blob.additional_stopped_threads.push(*thread_id);
                    }
                    self.blob.all_threads_stopped |= *all_threads_stopped;
                }
            }
            Event::SessionEnded { reason, .. } => {
                if let EndReason::Exited { exit_code } = reason {
                    self.blob.exit_code = *exit_code;
                }
            }
            // Latest wins, per breakpoint. codelldb sends two `breakpoint`
            // events about twenty milliseconds apart for a single
            // `setBreakpoints` — verified live — and a list that repeats the
            // same breakpoint's state twice invites a reader to think it
            // changed twice. What a caller wants is where each one ended up.
            Event::BreakpointUpdated { breakpoint, .. } => {
                let same = |existing: &lazydap_core::AdapterBreakpoint| {
                    match (existing.adapter_id, breakpoint.adapter_id) {
                        (Some(existing), Some(incoming)) => existing == incoming,
                        // No adapter id to match on: fall back to ours, and
                        // keep both if neither is identifiable.
                        _ => existing.id.is_some() && existing.id == breakpoint.id,
                    }
                };
                match self.blob.breakpoint_updates.iter().position(same) {
                    Some(index) => self.blob.breakpoint_updates[index] = breakpoint.clone(),
                    None => self.blob.breakpoint_updates.push(breakpoint.clone()),
                }
            }
            Event::ThreadChanged { update, .. } => self.blob.thread_updates.push(update.clone()),
            // `WatchUpdated` belongs to the project rather than to this run, so
            // it is not part of what a `--wait` saw. It cannot actually reach
            // here — the caller filters on `session_id`, which is `None` for
            // one — and is listed so that adding an event variant stays a
            // decision made here rather than absorbed by a catch-all.
            Event::SessionStarted { .. } | Event::Continued { .. } | Event::WatchUpdated { .. } => {
            }
        }
    }

    /// Keep listening briefly for the other threads of a multi-threaded stop.
    ///
    /// DAP sets `allThreadsStopped` on the first event only, and the rest
    /// follow in a burst. Returning on the first and then reporting a
    /// single-threaded pause would misdescribe the program.
    async fn coalesce(&mut self) {
        let until = Instant::now() + COALESCE_WINDOW;
        while let Ok(Ok(sequenced)) = tokio::time::timeout_at(until, self.events.recv()).await {
            if sequenced.seq <= self.watermark
                || sequenced.event.session_id() != Some(self.session.id)
            {
                continue;
            }
            self.watermark = sequenced.seq;
            self.absorb(&sequenced.event);
        }
    }

    /// Look for an exit code that is still on its way.
    ///
    /// The pump records a late `exited` on the session whatever else has
    /// happened, so this watches the session rather than the event stream —
    /// the ending has already been broadcast by the time we get here.
    async fn grace_for_exit_code(&mut self) {
        let until = Instant::now() + EXIT_CODE_GRACE;
        while Instant::now() < until {
            if let Some(exit_code) = self.session.exit_code() {
                self.blob.exit_code = Some(exit_code);
                // Knowing the process's status means it exited, whatever
                // order the adapter announced things in. Reporting
                // "terminated" alongside an exit code would be a distinction
                // with no meaning for the caller.
                if self.blob.state == WaitOutcome::Terminated {
                    self.blob.state = WaitOutcome::Exited;
                }
                return;
            }
            tokio::time::sleep(EXIT_CODE_POLL).await;
        }
    }

    /// Fetch the frame the program is sitting in.
    ///
    /// A convenience the blueprint asks for: the overwhelmingly common next
    /// question after "it stopped" is "where?", and making every caller spend
    /// a second round trip on it is the sort of thing that gets a debugger
    /// called clunky. A failure here is not the caller's problem — the stop
    /// itself is still true — so it is logged and left out.
    async fn fetch_top_frame(&mut self) {
        let Some(thread_id) = self
            .blob
            .thread_id
            .or_else(|| self.session.last_thread_id())
        else {
            return;
        };

        match self
            .session
            .adapter()
            .stack_trace(thread_id, Some(0), Some(1))
            .await
        {
            Ok((frames, _)) => self.blob.frame = frames.into_iter().next(),
            Err(error) => tracing::debug!(
                target: "daemon.session",
                session_id = %self.session.id,
                %error,
                "could not fetch the top frame for a wait",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::AdapterHandle;
    use lazydap_core::PauseReason;
    use lazydap_core::{
        AdapterBreakpoint, AdapterKind, BreakpointId, OutputCategory, OutputChunk, SessionId,
        SessionState,
    };
    use std::path::PathBuf;

    /// A session with no adapter behind it. Everything below exercises the
    /// event arithmetic, which is where the subtle bugs live; the adapter
    /// round trips are covered against real codelldb in `tests/`.
    fn session() -> Arc<Session> {
        let (event_tx, _keep_open) = tokio::sync::broadcast::channel(64);
        Arc::new(Session::new(
            SessionId::new(),
            AdapterKind::Codelldb,
            PathBuf::from("/tmp/hello"),
            SessionState::Running,
            AdapterHandle::detached(),
            event_tx,
        ))
    }

    fn output(session: &Arc<Session>, text: &str) -> Event {
        Event::Output {
            session_id: session.id,
            chunk: OutputChunk::new(OutputCategory::Stdout, text),
        }
    }

    fn stopped(session: &Arc<Session>, thread_id: i64, all_threads: bool) -> Event {
        Event::Stopped {
            session_id: session.id,
            thread_id: Some(thread_id),
            adapter_thread_id: None,
            reason: PauseReason::Breakpoint,
            raw_reason: None,
            all_threads_stopped: all_threads,
            hit_breakpoint_ids: vec![BreakpointId(1)],
        }
    }

    fn options(timeout_ms: u64) -> WaitOptions {
        WaitOptions {
            timeout: Some(Duration::from_millis(timeout_ms)),
            all_threads: false,
        }
    }

    #[tokio::test]
    async fn a_stop_ends_the_wait_and_names_what_caused_it() {
        let session = session();
        let wait = Wait::begin(&session);

        session.emit(output(&session, "hello\n"));
        session.emit(stopped(&session, 1, true));

        let blob = wait.collect(options(2_000)).await;
        assert_eq!(blob.state, WaitOutcome::Paused);
        assert_eq!(blob.reason, Some(PauseReason::Breakpoint));
        assert_eq!(blob.thread_id, Some(1));
        assert_eq!(blob.hit_breakpoint_ids, vec![BreakpointId(1)]);
        assert_eq!(blob.captured_output.len(), 1);
    }

    #[tokio::test]
    async fn output_from_before_the_wait_started_is_still_reported() {
        // The `stop_on_entry` case: the program printed during launch, and
        // the first `continue --wait` is the only thing that will ever read
        // that output.
        let session = session();
        session.emit(output(&session, "printed during launch\n"));

        let wait = Wait::begin(&session);
        session.emit(stopped(&session, 1, true));

        let blob = wait.collect(options(2_000)).await;
        assert_eq!(blob.captured_output.len(), 1);
        assert_eq!(blob.captured_output[0].output, "printed during launch\n");
    }

    #[tokio::test]
    async fn an_event_racing_the_subscription_is_reported_once_not_twice() {
        // `begin` subscribes and *then* drains the buffer, so anything landing
        // between those two steps is in both. The watermark is what stops it
        // being counted twice.
        let session = session();
        let events = session.subscribe();
        let wait = Wait {
            session: Arc::clone(&session),
            events,
            watermark: 0,
            started: Instant::now(),
            blob: StableState::new(WaitOutcome::Timeout),
        };

        // Emitted after the subscription: it is both broadcast and buffered.
        session.emit(output(&session, "raced\n"));
        let (pending, watermark, _) = session.undelivered();
        let mut wait = wait;
        for event in pending {
            wait.absorb_backlog(&event);
        }
        wait.watermark = watermark;

        session.emit(stopped(&session, 1, true));
        let blob = wait.collect(options(2_000)).await;

        assert_eq!(
            blob.captured_output.len(),
            1,
            "got: {:?}",
            blob.captured_output,
        );
    }

    #[tokio::test]
    async fn a_second_wait_does_not_re_report_what_the_first_one_already_carried() {
        // Caught live: the first `continue --wait` reported "hello", and so
        // did the second, because only the *buffer* drain marked events
        // delivered — everything a wait consumed live stayed undelivered.
        let session = session();

        let first = Wait::begin(&session);
        session.emit(output(&session, "hello\n"));
        session.emit(stopped(&session, 1, true));
        let first = first.collect(options(2_000)).await;
        assert_eq!(first.captured_output.len(), 1);

        let second = Wait::begin(&session);
        session.emit(output(&session, "goodbye\n"));
        session.emit(stopped(&session, 1, true));
        let second = second.collect(options(2_000)).await;

        let texts: Vec<&str> = second
            .captured_output
            .iter()
            .map(|chunk| chunk.output.as_str())
            .collect();
        assert_eq!(texts, vec!["goodbye\n"], "got: {texts:?}");
    }

    #[tokio::test]
    async fn a_clean_exit_carries_its_status_code() {
        let session = session();
        let wait = Wait::begin(&session);

        session.emit(output(&session, "goodbye\n"));
        session.set_exit_code(Some(0));
        session.end_once(EndReason::Exited { exit_code: Some(0) });

        let blob = wait.collect(options(2_000)).await;
        assert_eq!(blob.state, WaitOutcome::Exited);
        assert_eq!(blob.exit_code, Some(0));
        assert!(blob.frame.is_none(), "an exited program has no frame");
    }

    #[tokio::test]
    async fn a_termination_that_learns_its_exit_code_late_still_reports_it() {
        // DAP does not order `exited` before `terminated`. Without the grace
        // window the one blob the agent reads has no exit code in it.
        let session = session();
        let wait = Wait::begin(&session);

        session.end_once(EndReason::Terminated);
        let late = Arc::clone(&session);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            late.set_exit_code(Some(3));
        });

        let blob = wait.collect(options(2_000)).await;
        assert_eq!(blob.exit_code, Some(3), "the late code has to land");
        assert_eq!(
            blob.state,
            WaitOutcome::Exited,
            "knowing the status means it exited, whatever order it was said in",
        );
    }

    #[tokio::test]
    async fn a_program_that_never_stops_times_out_and_is_left_running() {
        let session = session();
        let wait = Wait::begin(&session);
        session.emit(output(&session, "still going\n"));

        let blob = wait.collect(options(60)).await;

        assert_eq!(blob.state, WaitOutcome::Timeout);
        assert_eq!(
            blob.captured_output.len(),
            1,
            "a timeout still reports what it saw",
        );
        assert!(
            session.state().is_live(),
            "a timeout must not pause the program behind the caller's back",
        );
    }

    #[tokio::test]
    async fn a_dead_adapter_ends_the_wait_rather_than_burning_the_timeout() {
        let session = session();
        let wait = Wait::begin(&session);

        session.end_once(EndReason::AdapterDied {
            detail: "eof".to_string(),
        });

        let blob = wait.collect(options(5_000)).await;
        assert_eq!(blob.state, WaitOutcome::AdapterDied);
    }

    #[tokio::test]
    async fn threads_stopping_just_behind_the_first_are_collected() {
        let session = session();
        let wait = Wait::begin(&session);

        session.emit(stopped(&session, 1, true));
        let others = Arc::clone(&session);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            others.emit(stopped(&others, 2, false));
            others.emit(stopped(&others, 3, false));
        });

        let blob = wait.collect(options(2_000)).await;
        assert_eq!(blob.thread_id, Some(1), "the first stop names the blob");
        assert_eq!(blob.additional_stopped_threads, vec![2, 3]);
    }

    #[tokio::test]
    async fn all_threads_waits_for_the_snapshot_it_was_asked_for() {
        let session = session();
        let wait = Wait::begin(&session);

        session.emit(stopped(&session, 1, false));
        let rest = Arc::clone(&session);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            rest.emit(stopped(&rest, 2, true));
        });

        let blob = wait
            .collect(WaitOptions {
                timeout: Some(Duration::from_millis(2_000)),
                all_threads: true,
            })
            .await;

        assert_eq!(blob.state, WaitOutcome::Paused);
        assert!(
            blob.all_threads_stopped,
            "the point of --all-threads is not returning until this is true",
        );
    }

    #[tokio::test]
    async fn breakpoint_and_thread_changes_during_the_run_are_reported() {
        let session = session();
        let wait = Wait::begin(&session);

        session.emit(Event::BreakpointUpdated {
            session_id: Some(session.id),
            breakpoint: AdapterBreakpoint {
                id: Some(BreakpointId(1)),
                adapter_id: Some(1),
                verified: true,
                line: Some(21),
                message: None,
            },
        });
        session.emit(Event::ThreadChanged {
            session_id: session.id,
            update: lazydap_core::ThreadUpdate {
                thread_id: 2,
                kind: lazydap_core::ThreadUpdateKind::Started,
                name: None,
            },
        });
        session.emit(stopped(&session, 1, true));

        let blob = wait.collect(options(2_000)).await;
        assert_eq!(blob.breakpoint_updates.len(), 1);
        assert!(blob.breakpoint_updates[0].verified);
        assert_eq!(blob.thread_updates.len(), 1);
    }

    #[tokio::test]
    async fn one_breakpoint_is_reported_once_however_often_the_adapter_mentions_it() {
        // codelldb sends two `breakpoint` events for one `setBreakpoints`,
        // about 20ms apart. Verified live on 2026-07-30.
        let session = session();
        let wait = Wait::begin(&session);

        for line in [19, 21] {
            session.emit(Event::BreakpointUpdated {
                session_id: Some(session.id),
                breakpoint: AdapterBreakpoint {
                    id: Some(BreakpointId(1)),
                    adapter_id: Some(1),
                    verified: true,
                    line: Some(line),
                    message: None,
                },
            });
        }
        session.emit(stopped(&session, 1, true));

        let blob = wait.collect(options(2_000)).await;
        assert_eq!(blob.breakpoint_updates.len(), 1);
        assert_eq!(
            blob.breakpoint_updates[0].line,
            Some(21),
            "where it ended up, not where it passed through",
        );
    }

    #[tokio::test]
    async fn two_different_breakpoints_are_both_reported() {
        let session = session();
        let wait = Wait::begin(&session);

        for adapter_id in [1, 2] {
            session.emit(Event::BreakpointUpdated {
                session_id: Some(session.id),
                breakpoint: AdapterBreakpoint {
                    id: Some(BreakpointId(adapter_id as u32)),
                    adapter_id: Some(adapter_id),
                    verified: true,
                    line: Some(10),
                    message: None,
                },
            });
        }
        session.emit(stopped(&session, 1, true));

        let blob = wait.collect(options(2_000)).await;
        assert_eq!(blob.breakpoint_updates.len(), 2);
    }

    #[tokio::test]
    async fn a_flood_of_output_is_capped_and_says_so() {
        let session = session();
        let wait = Wait::begin(&session);

        let chunk = "x".repeat(100_000);
        for _ in 0..15 {
            session.emit(output(&session, &chunk));
        }
        session.emit(stopped(&session, 1, true));

        let blob = wait.collect(options(2_000)).await;
        assert!(
            blob.output_truncated,
            "a caller must be able to tell a full picture from a partial one",
        );
        let captured: usize = blob
            .captured_output
            .iter()
            .map(|chunk| chunk.output.len())
            .sum();
        assert!(captured <= OUTPUT_CAP_BYTES, "got: {captured}");
    }

    #[tokio::test]
    async fn what_survives_the_cap_is_a_prefix_of_what_the_program_printed() {
        let session = session();
        let wait = Wait::begin(&session);

        // Big lines until the cap is reached, then a small one that would
        // still fit under it. Skipping only the overrunning chunk let this
        // marker through, and `captured_output` became a *splice*: half a
        // megabyte missing from the middle with the tail glued onto the cut,
        // under a flag every reader takes to mean "the tail was cut".
        let line = "x".repeat(100_000);
        for _ in 0..15 {
            session.emit(output(&session, &line));
        }
        session.emit(output(&session, "MARKER-AFTER-THE-CAP\n"));
        session.emit(stopped(&session, 1, true));

        let blob = wait.collect(options(2_000)).await;
        let captured: String = blob
            .captured_output
            .iter()
            .map(|chunk| chunk.output.as_str())
            .collect();

        assert!(blob.output_truncated);
        assert!(
            !captured.contains("MARKER-AFTER-THE-CAP"),
            "output produced after the cap was reached is not a prefix of anything",
        );
        let printed = line.repeat(15) + "MARKER-AFTER-THE-CAP\n";
        assert!(
            printed.starts_with(&captured),
            "what a caller keeps must be a strict prefix of what ran: kept {} bytes",
            captured.len(),
        );
    }

    #[tokio::test]
    async fn output_lost_before_the_wait_started_is_reported_as_missing() {
        // The other half of D070. A debuggee that prints more than the session
        // buffer holds between two CLI invocations pushes the *beginning* of
        // its own output out of the buffer before `continue --wait` is called.
        // The blob then carried a suffix and said `output_truncated: false` —
        // the same lie as a spliced middle, from the other end (D072).
        let session = session();
        for line in 0..1_200 {
            session.emit(output(&session, &format!("line {line}\n")));
        }

        let wait = Wait::begin(&session);
        session.emit(stopped(&session, 1, true));
        let blob = wait.collect(options(2_000)).await;

        assert!(
            blob.output_truncated,
            "the beginning of the run is gone, so the caller is not seeing all of it",
        );
        assert!(
            blob.dropped_events > 0,
            "and how much is gone has to be sayable: {}",
            blob.dropped_events,
        );

        let kept: String = blob
            .captured_output
            .iter()
            .map(|chunk| chunk.output.as_str())
            .collect();
        assert!(
            !kept.contains("line 0\n"),
            "the premise of the test is that the start was lost",
        );
    }

    #[tokio::test]
    async fn a_wait_that_lost_nothing_says_so() {
        let session = session();
        let wait = Wait::begin(&session);

        session.emit(output(&session, "all of it\n"));
        session.emit(stopped(&session, 1, true));

        let blob = wait.collect(options(2_000)).await;
        assert!(!blob.output_truncated);
        assert_eq!(blob.dropped_events, 0);
    }

    #[tokio::test]
    async fn a_loss_already_reported_is_not_reported_again() {
        // The count resets when a wait commits delivery, or every later blob
        // in the session repeats a gap that has already been accounted for.
        let session = session();
        for line in 0..1_200 {
            session.emit(output(&session, &format!("line {line}\n")));
        }

        let first = Wait::begin(&session);
        session.emit(stopped(&session, 1, true));
        assert!(first.collect(options(2_000)).await.dropped_events > 0);

        let second = Wait::begin(&session);
        session.emit(stopped(&session, 1, true));
        let blob = second.collect(options(2_000)).await;

        assert_eq!(blob.dropped_events, 0, "the gap belonged to the first wait");
        assert!(!blob.output_truncated);
    }

    #[tokio::test]
    async fn another_session_s_events_are_not_this_session_s_business() {
        let session = session();
        let wait = Wait::begin(&session);

        // The broadcast is daemon-wide; the blob is not.
        let stranger = SessionId::new();
        session.emit(Event::Output {
            session_id: stranger,
            chunk: OutputChunk::new(OutputCategory::Stdout, "not ours\n"),
        });
        session.emit(stopped(&session, 1, true));

        let blob = wait.collect(options(2_000)).await;
        assert!(
            blob.captured_output.is_empty(),
            "got: {:?}",
            blob.captured_output,
        );
    }
}
