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
use lazydap_protocol::{Event, FrameLocals, StableState};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::Instant;

/// Resolves when the client that asked for a wait has gone away.
///
/// A `--wait` holds the session's execution permit for as long as it runs, so
/// one whose caller has hung up would keep every other client's `continue`
/// queued behind a request nobody is waiting for. The daemon hands the wait
/// this end of a channel it drops when the connection closes (D-WP3-5).
pub type Abandoned = tokio::sync::oneshot::Receiver<()>;

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

/// How far down the stack to look for a frame in the user's own code.
///
/// Deep enough for the case it is about — a handful of library frames between
/// the crash and the caller responsible for it — and bounded so a thousand-deep
/// recursion costs the adapter no more to answer than a shallow one. The whole
/// search is one `stackTrace`, so this is a size, not a number of round trips.
const USER_FRAME_SEARCH_DEPTH: u32 = 24;

/// Most locals a stop blob carries. Beyond it the list is a flagged prefix and
/// `lazydap variables` reads the rest (D078).
const STOP_LOCALS_CAP: usize = 100;

pub struct WaitOptions {
    /// `None` waits forever, which the caller has asked for explicitly.
    pub timeout: Option<Duration>,
    /// Wait for every thread to stop rather than returning on the first.
    pub all_threads: bool,
    /// Ends the wait when the client that asked for it hangs up. `None` for a
    /// caller with no connection behind it, such as a test.
    pub abandoned: Option<Abandoned>,
}

/// A wait that has already subscribed and is safe to send a request behind.
///
/// Begin it, send the DAP request, then collect. Doing it in that order is not
/// a convention — `begin` is the only way to make one of these, and it
/// subscribes on the way.
pub struct Wait {
    session: Arc<Session>,
    events: Receiver<SeqEvent>,
    /// Live events at or below this were already taken from the buffer.
    watermark: u64,
    started: Instant,
    blob: StableState,
    /// What `captured_output` currently weighs.
    ///
    /// Carried rather than recomputed: re-summing every chunk on every chunk
    /// is quadratic in the number of them, and a debuggee that prints a
    /// megabyte in small pieces made the wait task slow enough to fall behind
    /// the broadcast and lose the very stop it was waiting for (D-WP3-1).
    captured_bytes: usize,
    /// An outcome settled before the event loop ran.
    preempted: Option<WaitOutcome>,
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
            captured_bytes: 0,
            preempted: None,
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

    /// Take a stop or ending the session recorded after `since` as the answer.
    ///
    /// For the one caller that sends nothing: a `continue` on a program that
    /// is already running. Between deciding not to send and subscribing, the
    /// program can reach the very stop the caller is waiting for — the
    /// subscription is too late to see it and [`Self::absorb_backlog`]
    /// deliberately will not fold a stop in, so the wait ran to its timeout
    /// while `status` said `paused`.
    ///
    /// `since` is where the session's event history stood before the decision,
    /// and it is what makes this safe rather than the bug the backlog rule
    /// exists to prevent: a stop at or below it is the one the program was
    /// already sitting at, and reporting *that* would answer every `continue`
    /// with the reason the previous one stopped for (D-WP3-3).
    pub fn adopt_ending_since(&mut self, since: u64, all_threads: bool) {
        let Some(event) = self
            .session
            .events_since(since)
            .into_iter()
            .rev()
            .find(|sequenced| {
                matches!(
                    sequenced.event,
                    Event::Stopped { .. } | Event::SessionEnded { .. }
                )
            })
            .map(|sequenced| sequenced.event)
        else {
            return;
        };
        self.preempted = self.consider(&event, all_threads);
    }

    /// Block until the program settles, and describe what happened.
    pub async fn collect(mut self, mut options: WaitOptions) -> StableState {
        let deadline = options.timeout.map(|timeout| self.started + timeout);

        let outcome = match self.preempted {
            Some(outcome) => Some(outcome),
            None => {
                self.run(deadline, options.all_threads, options.abandoned.as_mut())
                    .await
            }
        };
        let Some(outcome) = outcome else {
            // The client hung up. Nothing is committed on the way out: the
            // events this wait consumed are still nobody's, so the next wait —
            // from whatever client comes along — still reports them. The blob
            // is built only because the signature promises one; the connection
            // it would go to is already gone (D-WP3-5).
            self.blob.state = WaitOutcome::Timeout;
            self.blob.elapsed_ms = self.started.elapsed().as_millis() as u64;
            return self.blob;
        };
        self.blob.state = outcome;

        if outcome == WaitOutcome::Paused {
            if !options.all_threads {
                self.coalesce().await;
            }
            self.fetch_stop_context().await;
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

    /// Read events until one of them ends the wait, or nobody is left to tell.
    ///
    /// `None` means the client hung up: no outcome, and nothing to report.
    async fn run(
        &mut self,
        deadline: Option<Instant>,
        all_threads: bool,
        mut abandoned: Option<&mut Abandoned>,
    ) -> Option<WaitOutcome> {
        loop {
            // `recv` is cancellation-safe — a losing branch has received
            // nothing — so racing the hang-up against it costs no events.
            let received = match abandoned.as_deref_mut() {
                Some(gone) => tokio::select! {
                    biased;
                    _ = gone => return None,
                    received = next(&mut self.events, deadline) => received,
                },
                None => next(&mut self.events, deadline).await,
            };
            // The program is still running. We do not pause it: an automatic
            // pause on timeout would be a side effect nobody asked for, and
            // can mask the very hang being diagnosed.
            let Some(received) = received else {
                return Some(WaitOutcome::Timeout);
            };

            let sequenced = match received {
                Ok(sequenced) => sequenced,
                // Slower than the daemon: some events are gone for good. Say
                // so rather than presenting a gap as the whole story — and
                // then find out what the gap contained, because the buffer
                // outlives the broadcast's backlog and the dropped range can
                // hold the stop this wait exists to return (D-WP3-1).
                Err(RecvError::Lagged(missed)) => {
                    tracing::warn!(target: "daemon.session", missed, "a wait fell behind its events");
                    self.record_loss(missed);
                    if let Some(outcome) = self.reconcile(all_threads) {
                        return Some(outcome);
                    }
                    continue;
                }
                // The session's sender is gone, which only happens once
                // everything holding it has been dropped.
                Err(RecvError::Closed) => return Some(WaitOutcome::AdapterDied),
            };

            if sequenced.seq <= self.watermark
                || sequenced.event.session_id() != Some(self.session.id)
            {
                continue;
            }
            self.watermark = sequenced.seq;

            if let Some(outcome) = self.consider(&sequenced.event, all_threads) {
                return Some(outcome);
            }
        }
    }

    /// Catch up from the session's own buffer after falling behind.
    ///
    /// The broadcast drops the oldest events for a slow subscriber; the
    /// session's ring buffer does not lose them at the same moment, and it is
    /// the only remaining record of what happened. Reading it forward from the
    /// watermark is what stops a `--wait` reporting `timeout` for a program
    /// that has already stopped or exited — the wait blocked while `status`
    /// said `paused`, which is worse than a slow answer because the caller
    /// cannot tell it is wrong.
    ///
    /// Stops at the first event that settles the wait: everything behind it is
    /// still in the buffer, still undelivered, and belongs to whatever comes
    /// next rather than to this blob.
    fn reconcile(&mut self, all_threads: bool) -> Option<WaitOutcome> {
        for sequenced in self.session.events_since(self.watermark) {
            if sequenced.event.session_id() != Some(self.session.id) {
                continue;
            }
            self.watermark = sequenced.seq;
            if let Some(outcome) = self.consider(&sequenced.event, all_threads) {
                return Some(outcome);
            }
        }
        None
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
                if self.captured_bytes + chunk.output.len() > OUTPUT_CAP_BYTES {
                    self.blob.output_truncated = true;
                } else {
                    self.captured_bytes += chunk.output.len();
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

    /// Fetch where the program is, whose code that is, and what is in scope.
    ///
    /// A convenience the blueprint asks for: the overwhelmingly common next
    /// question after "it stopped" is "where?", and making every caller spend
    /// a second round trip on it is the sort of thing that gets a debugger
    /// called clunky. Two more questions turned out to be just as common and
    /// just as reliably asked, so they are answered here too (D078) — the
    /// nearest frame in the user's own code, which the top frame is not when a
    /// program dies inside libc, and the top frame's locals.
    ///
    /// A failure anywhere here is not the caller's problem — the stop itself is
    /// still true — so it is logged and the field left out. Absent is honest;
    /// an empty list would claim the frame has no locals when the truth is that
    /// nobody could find out.
    async fn fetch_stop_context(&mut self) {
        let Some(thread_id) = self
            .blob
            .thread_id
            .or_else(|| self.session.last_thread_id())
        else {
            return;
        };
        let fence = self.session.stop_generation();

        // One request, not one per frame: `user_frame` costs a longer answer to
        // the stack trace already being made, and nothing else. That is what
        // keeps it always-on rather than a flag.
        let frames = match self
            .session
            .adapter()
            .stack_trace(thread_id, Some(0), Some(USER_FRAME_SEARCH_DEPTH))
            .await
        {
            Ok((frames, _)) => frames,
            Err(error) => {
                tracing::debug!(
                    target: "daemon.session",
                    session_id = %self.session.id,
                    %error,
                    "could not fetch the top frame for a wait",
                );
                return;
            }
        };

        // The program moved while we were asking. Reporting these frames would
        // describe a moment it has left, and minting handles for them would
        // stamp them with a stop they did not come from.
        if !self.session.still_at(fence) {
            tracing::debug!(
                target: "daemon.session",
                session_id = %self.session.id,
                "the program moved before its stop context could be read",
            );
            return;
        }

        let Some(top) = frames.first().cloned() else {
            return;
        };
        // Only when the top frame cannot answer it itself. A `user_frame`
        // repeating `frame` would invite a reader to think the two had been
        // compared and found to differ.
        let responsible = match has_source_path(&top) {
            true => None,
            false => self.responsible_frame(&frames).await,
        };

        // The locals belong to whichever frame a person would look at, which is
        // the responsible one when there is one. A crash inside `strcmp` has no
        // locals worth reading in `strcmp`; the ones that explain it are in the
        // caller that passed the null. Reporting the top frame's empty list
        // there would have been true and useless, and would have cost the two
        // round trips this exists to save. `locals.frame_id` names which frame
        // it is, so the choice is never something a reader has to infer (D078).
        let locals_frame_id = responsible.as_ref().unwrap_or(&top).id;

        self.blob.user_frame = responsible.map(|frame| self.session.mint_frame(fence, frame));
        self.blob.frame = Some(self.session.mint_frame(fence, top));
        self.fetch_locals(fence, locals_frame_id).await;
    }

    /// The nearest frame in the user's own code, looking past the window if it
    /// has to.
    ///
    /// `frames` is the first [`USER_FRAME_SEARCH_DEPTH`] of the stack, which is
    /// enough for the case this is about — a handful of library frames between
    /// a crash and the caller responsible for it — and cheap for the deep
    /// recursion it is bounded against.
    ///
    /// When the window is *exhausted* without a hit, though, `None` would be
    /// two different answers wearing one face: "no frame in this stack has a
    /// source path" and "none of the first two dozen did". Only the first is
    /// something a reader can act on, so the ambiguous case costs one more
    /// request rather than a plausible-looking absence (D083). It is reached
    /// only by a stop two dozen library frames deep, which is rare, and never
    /// by an ordinary breakpoint — those stop in code with a path and never
    /// search at all.
    async fn responsible_frame(
        &self,
        frames: &[lazydap_core::StackFrame],
    ) -> Option<lazydap_core::StackFrame> {
        let thread_id = match search(frames, USER_FRAME_SEARCH_DEPTH) {
            Search::Found(frame) => return Some(frame),
            Search::Absent => return None,
            Search::LookFurther => self
                .blob
                .thread_id
                .or_else(|| self.session.last_thread_id())?,
        };

        match self
            .session
            .adapter()
            .stack_trace(thread_id, None, None)
            .await
        {
            Ok((whole, _)) => whole.into_iter().find(has_source_path),
            Err(error) => {
                tracing::debug!(
                    target: "daemon.session",
                    session_id = %self.session.id,
                    %error,
                    "could not search the whole stack for a frame in the user's code",
                );
                None
            }
        }
    }

    /// The top frame's locals, so reading one is not a second command.
    ///
    /// Two round trips — `scopes`, then `variables` on whichever of them is the
    /// local one. They are the two every caller was making by hand anyway
    /// (D078).
    async fn fetch_locals(&mut self, fence: u64, adapter_frame_id: i64) {
        let scopes = match self.session.adapter().scopes(adapter_frame_id).await {
            Ok(scopes) => scopes,
            Err(error) => {
                tracing::debug!(
                    target: "daemon.session",
                    session_id = %self.session.id,
                    %error,
                    "could not fetch the stopped frame's scopes",
                );
                return;
            }
        };

        // The locals, not every scope. `Registers` and `Global` are large,
        // rarely what anybody meant, and `expensive` exists to say "do not
        // fetch this without being asked".
        let Some(local) = scopes
            .iter()
            .find(|scope| !scope.expensive && scope.name.eq_ignore_ascii_case("local"))
            .or_else(|| scopes.iter().find(|scope| !scope.expensive))
        else {
            return;
        };
        let reference = local.variables_reference;

        // One more than the cap, so the transfer is bounded and "there is more
        // than you are seeing" is still decidable. Asking for everything let a
        // two-thousand-element frame cross the wire in full before the cap threw
        // it away — a cap on the reply is not a cap on the round trip (D083).
        let wanted = STOP_LOCALS_CAP as u32 + 1;
        let mut variables = match self
            .session
            .adapter()
            .variables(
                reference,
                lazydap_core::VariableFilter::All,
                None,
                Some(wanted),
            )
            .await
        {
            Ok(variables) => variables,
            Err(error) => {
                tracing::debug!(
                    target: "daemon.session",
                    session_id = %self.session.id,
                    %error,
                    "could not fetch the stopped frame's locals",
                );
                return;
            }
        };

        if !self.session.still_at(fence) {
            return;
        }

        // Same rule as `variables` applies to its own cap: what came back is one
        // row longer than the cap exactly when there is more (D083).
        let truncated = variables.len() > STOP_LOCALS_CAP;
        variables.truncate(STOP_LOCALS_CAP);
        for variable in &mut variables {
            variable.variables_reference = self
                .session
                .mint_variables_reference(fence, variable.variables_reference);
        }

        self.blob.locals = Some(FrameLocals {
            // Our handle for the frame these came from, which is `user_frame`'s
            // when there is one and `frame`'s otherwise. Both were minted a
            // moment ago, and one adapter number yields one handle per stop, so
            // this is the same number rather than a second name for it.
            frame_id: self.session.mint_frame_id(fence, adapter_frame_id),
            variables_reference: self.session.mint_variables_reference(fence, reference),
            variables,
            truncated,
        });
    }
}

/// The next event, or `None` when the deadline passed first.
///
/// Free rather than a method so it can be one arm of a `select!` while the
/// wait's own state stays borrowable.
async fn next(
    events: &mut Receiver<SeqEvent>,
    deadline: Option<Instant>,
) -> Option<Result<SeqEvent, RecvError>> {
    match deadline {
        Some(deadline) => tokio::time::timeout_at(deadline, events.recv()).await.ok(),
        None => Some(events.recv().await),
    }
}

/// What a search of the fetched window settled.
#[derive(Debug, PartialEq)]
enum Search {
    /// The nearest frame in the user's own code.
    Found(lazydap_core::StackFrame),
    /// The window held the whole stack and none of it had a source path, so
    /// the absence is the answer.
    Absent,
    /// The window ran out first, so absence would be two answers wearing one
    /// face — "no frame has a path" and "none of the first two dozen did".
    LookFurther,
}

/// Look for a frame in the user's code among the `depth` frames fetched.
///
/// Split out from the fetching so the *decision* — and particularly the
/// distinction between a settled absence and an exhausted window — is testable
/// without an adapter (D083).
fn search(frames: &[lazydap_core::StackFrame], depth: u32) -> Search {
    match frames.iter().find(|frame| has_source_path(frame)) {
        Some(frame) => Search::Found(frame.clone()),
        None if frames.len() < depth as usize => Search::Absent,
        None => Search::LookFurther,
    }
}

/// Whether a frame names a file on disk somebody could open.
///
/// A `source` with only a `source_reference` is the adapter offering to send
/// bytes for something that has no path — libc, a JIT frame — and is exactly
/// the case `user_frame` exists for.
fn has_source_path(frame: &lazydap_core::StackFrame) -> bool {
    frame
        .source
        .as_ref()
        .is_some_and(|source| source.path.is_some())
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
    use std::sync::atomic::AtomicU64;

    /// A session with no adapter behind it. Everything below exercises the
    /// event arithmetic, which is where the subtle bugs live; the adapter
    /// round trips are covered against real codelldb in `tests/`.
    ///
    /// The broadcast is the daemon's own size, not a convenient small one. The
    /// lag tests below are *about* the relationship between this channel and
    /// the session's ring buffer, and a 64-slot channel against a 4096-event
    /// ring is a ratio production never has — it made those tests pass against
    /// a buffer that could not have recovered anything (D-WP3-1).
    fn session() -> Arc<Session> {
        let (event_tx, _keep_open) =
            tokio::sync::broadcast::channel(crate::state::EVENT_CHANNEL_CAPACITY);
        Arc::new(Session::new(
            SessionId::new(),
            AdapterKind::Codelldb,
            PathBuf::from("/tmp/hello"),
            SessionState::Running,
            AdapterHandle::detached(),
            event_tx,
            Arc::new(AtomicU64::new(0)),
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

    /// A frame with a source path, or one without — a library frame carries a
    /// `source_reference` and no file anybody can open.
    fn frame(named: bool) -> lazydap_core::StackFrame {
        lazydap_core::StackFrame {
            id: 1,
            name: "f".to_string(),
            line: 1,
            column: 0,
            source: Some(lazydap_core::SourceRef {
                name: Some("f".to_string()),
                path: named.then(|| PathBuf::from("/tmp/f.c")),
                source_reference: (!named).then_some(1000),
            }),
        }
    }

    #[test]
    fn a_window_holding_the_whole_stack_settles_the_question() {
        // Fewer frames than the window means the stack ended, so "no frame in
        // the user's code" is a fact rather than a limit of how far we looked.
        assert_eq!(search(&[frame(false), frame(false)], 24), Search::Absent);
    }

    #[test]
    fn a_window_that_ran_out_without_a_hit_is_not_an_answer_yet() {
        // The ambiguity this exists to remove: `user_frame: null` would mean
        // both "no frame has a source path" and "none of the first two dozen
        // did", and only the first is something a reader can act on (D083).
        let frames: Vec<_> = (0..24).map(|_| frame(false)).collect();
        assert_eq!(search(&frames, 24), Search::LookFurther);
    }

    #[test]
    fn the_nearest_frame_with_a_path_wins_however_deep_the_window_went() {
        let frames = vec![frame(false), frame(false), frame(true)];
        assert!(matches!(search(&frames, 24), Search::Found(_)));
    }

    fn options(timeout_ms: u64) -> WaitOptions {
        WaitOptions {
            timeout: Some(Duration::from_millis(timeout_ms)),
            all_threads: false,
            abandoned: None,
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
            captured_bytes: 0,
            preempted: None,
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

    /// DAP does not order `exited` before `terminated`. Without the grace
    /// window the one blob the agent reads has no exit code in it.
    ///
    /// **Time is controlled rather than raced.** This used to sleep 30ms of
    /// real time against a 250ms real grace window and failed once under load —
    /// the machine was busy enough that 250ms of wall clock passed before the
    /// 30ms sleep was scheduled. The margin was never the point of the test,
    /// and a test that fails under load teaches people to re-run rather than to
    /// read. Under `start_paused` the clock only advances when every task is
    /// idle, so the ordering below is a fact about the code rather than about
    /// the machine: the setter fires at 25ms, and the grace loop — which polls
    /// every 10ms for 250ms — next looks at 30ms and finds it. Load cannot
    /// change either number.
    #[tokio::test(start_paused = true)]
    async fn a_termination_that_learns_its_exit_code_late_still_reports_it() {
        let session = session();
        let wait = Wait::begin(&session);

        session.end_once(EndReason::Terminated);
        let late = Arc::clone(&session);
        tokio::spawn(async move {
            // Deliberately not a multiple of EXIT_CODE_POLL: landing exactly on
            // a poll instant would make the answer depend on which of two
            // ready tasks the scheduler picked first, which is the sort of
            // coin-toss this test was rewritten to remove.
            tokio::time::sleep(Duration::from_millis(25)).await;
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
                abandoned: None,
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
        for line in 0..(crate::state::EVENT_BUFFER_CAPACITY + 200) {
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
        for line in 0..(crate::state::EVENT_BUFFER_CAPACITY + 200) {
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
    async fn the_carried_total_fills_the_cap_to_the_byte() {
        // The total is carried rather than re-summed, because re-summing every
        // chunk on every chunk is quadratic and a debuggee printing a megabyte
        // in small pieces made the wait slow enough to fall behind its own
        // events (D-WP3-1). Carrying it has to be exact: fifty of these fit
        // the cap precisely, and the fifty-first is what trips it.
        let session = session();
        let wait = Wait::begin(&session);

        let chunk = "y".repeat(20_000);
        for _ in 0..60 {
            session.emit(output(&session, &chunk));
        }
        session.emit(stopped(&session, 1, true));

        let blob = wait.collect(options(5_000)).await;
        let captured: usize = blob
            .captured_output
            .iter()
            .map(|chunk| chunk.output.len())
            .sum();
        assert_eq!(captured, OUTPUT_CAP_BYTES, "got: {captured}");
        assert_eq!(blob.captured_output.len(), 50);
        assert!(blob.output_truncated);
    }

    #[tokio::test]
    async fn a_stop_that_fell_out_of_the_broadcast_still_ends_the_wait() {
        // The failure this exists for: the wait falls behind, the dropped
        // range holds the `stopped`, and the blob comes back `timeout` while
        // `lazydap status` says `paused` — a lie the caller cannot detect.
        // The session's own buffer still has the stop, and reading it is the
        // difference between an answer and a wedged agent (D-WP3-1).
        let session = session();
        let wait = Wait::begin(&session);

        session.emit(stopped(&session, 1, true));
        // Past the broadcast's capacity, so the stop is certainly outside the
        // window tokio rewinds the receiver to — and inside the ring buffer,
        // which is four times the size. Nothing polls the receiver until
        // `collect`, so the lag is a certainty rather than a race.
        for line in 0..(crate::state::EVENT_CHANNEL_CAPACITY + 200) {
            session.emit(output(&session, &format!("line {line}\n")));
        }

        let blob = wait.collect(options(2_000)).await;
        assert_eq!(blob.state, WaitOutcome::Paused, "got: {:?}", blob.state);
        assert_eq!(blob.reason, Some(PauseReason::Breakpoint));
        assert!(
            blob.dropped_events > 0,
            "and the gap is still declared: {}",
            blob.dropped_events,
        );
    }

    #[tokio::test]
    async fn an_ending_that_fell_out_of_the_broadcast_still_ends_the_wait() {
        let session = session();
        let wait = Wait::begin(&session);

        session.set_exit_code(Some(0));
        session.end_once(EndReason::Exited { exit_code: Some(0) });
        for line in 0..(crate::state::EVENT_CHANNEL_CAPACITY + 200) {
            session.emit(output(&session, &format!("line {line}\n")));
        }

        let blob = wait.collect(options(2_000)).await;
        assert_eq!(blob.state, WaitOutcome::Exited, "got: {:?}", blob.state);
        assert_eq!(blob.exit_code, Some(0));
    }

    #[tokio::test]
    async fn a_stop_reached_before_the_subscription_is_this_run_s_answer() {
        // The already-running `continue`: nothing is sent, so the only record
        // of a stop reached between that decision and the subscription is the
        // session's buffer (D-WP3-3).
        let session = session();
        let before = session.event_watermark();
        session.emit(stopped(&session, 7, true));

        let mut wait = Wait::begin(&session);
        wait.adopt_ending_since(before, false);

        let blob = wait.collect(options(60_000)).await;
        assert_eq!(blob.state, WaitOutcome::Paused);
        assert_eq!(blob.thread_id, Some(7));
    }

    #[tokio::test]
    async fn the_stop_a_run_began_at_is_not_the_stop_it_ended_at() {
        // The other half of the rule, and the reason `absorb_backlog` will not
        // fold a stop in on its own: the program was sitting at a stop nobody
        // had reported, and answering this `continue` with *that* would say
        // every run stopped for the reason the previous one did (D-WP3-3).
        let session = session();
        session.emit(stopped(&session, 7, true));
        let before = session.event_watermark();

        let mut wait = Wait::begin(&session);
        wait.adopt_ending_since(before, false);

        let blob = wait.collect(options(60)).await;
        assert_eq!(
            blob.state,
            WaitOutcome::Timeout,
            "a stop at or below the mark belongs to the previous run",
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_wait_whose_client_hung_up_ends_at_once_and_commits_nothing() {
        // `continue --wait --timeout 0` holds the session's execution permit
        // for as long as it runs, so a Ctrl-C used to leave every later
        // `continue` from every client queued behind a caller that was not
        // there. Nothing is marked delivered on the way out: the output this
        // wait saw is still nobody's (D-WP3-5).
        let session = session();
        let wait = Wait::begin(&session);
        session.emit(output(&session, "printed before the hang-up\n"));

        let (hangup, abandoned) = tokio::sync::oneshot::channel::<()>();
        drop(hangup);
        let blob = wait
            .collect(WaitOptions {
                timeout: None,
                all_threads: false,
                abandoned: Some(abandoned),
            })
            .await;
        assert_eq!(blob.state, WaitOutcome::Timeout);

        let next = Wait::begin(&session);
        session.emit(stopped(&session, 1, true));
        let next = next.collect(options(2_000)).await;
        assert_eq!(
            next.captured_output.len(),
            1,
            "the abandoned wait must not have consumed anybody's output: {:?}",
            next.captured_output,
        );
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
