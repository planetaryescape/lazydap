//! The numbers lazydap hands out for a frame and for a set of variables.
//!
//! Both used to be the adapter's own. That is the bug: an adapter's frame id
//! and `variablesReference` are valid only until the program moves, and it is
//! free to hand the *same number* out again at the next stop for something
//! else entirely. A caller holding one across a `continue` therefore had two
//! futures, and the quiet one was much the worse:
//!
//! - the number addresses nothing any more, and the adapter says so in its own
//!   words — `Invalid frame reference: 0`, or `can't evaluate expressions when
//!   the process is running`, which is not even true;
//! - the number has been **recycled**, and the adapter answers it. Somebody
//!   else's variables come back under exit 0, with nothing anywhere in the
//!   response to say the question was about a moment that has passed.
//!
//! So lazydap mints its own. A handle is issued once, never reused, and belongs
//! to the stop generation it was issued at (D059's fence, which already counts
//! exactly this). Presenting one from an older generation is refused here,
//! before the adapter is asked anything — and because handles are never
//! recycled, "older generation" is always decidable. That is what closes the
//! collision: there is no number a caller can hold that means one thing to them
//! and another to the adapter.

use lazydap_protocol::{ErrorCode, IpcError};
use std::collections::HashMap;

/// Which kind of thing a handle addresses. They are numbered from one sequence
/// so a handle is never ambiguous, and kept apart so a frame id presented as a
/// variables reference is refused rather than silently resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleKind {
    Frame,
    Variables,
}

impl HandleKind {
    fn noun(self) -> &'static str {
        match self {
            Self::Frame => "frame id",
            Self::Variables => "variables reference",
        }
    }

    /// The command that hands this kind out. Every refusal names it, because
    /// "that handle is no good" without "here is where a good one comes from"
    /// is half an error message.
    fn source(self) -> &'static str {
        match self {
            Self::Frame => "`lazydap stack`",
            Self::Variables => "`lazydap scopes` or `lazydap variables`",
        }
    }
}

/// One generation's worth of handles, both ways round.
#[derive(Default)]
struct Side {
    to_adapter: HashMap<i64, i64>,
    to_ours: HashMap<i64, i64>,
}

impl Side {
    fn clear(&mut self) {
        self.to_adapter.clear();
        self.to_ours.clear();
    }
}

/// Every handle this session has outstanding, and the stop they belong to.
pub struct HandleTable {
    /// The stop generation `frames` and `variables` describe.
    generation: u64,
    /// The next handle to issue. Monotonic for the life of the session: a
    /// handle is never reused, which is the whole point.
    next: i64,
    /// The highest handle issued before `generation`. Anything at or below it
    /// that is not in the current maps was issued at an earlier stop, which is
    /// what lets a refusal say *stale* rather than merely *unknown*.
    stale_floor: i64,
    frames: Side,
    variables: Side,
}

impl Default for HandleTable {
    fn default() -> Self {
        Self {
            generation: 0,
            // From one, so no handle is ever `0`. A `variables_reference` of
            // zero means "this is a scalar, do not try to expand it", and a
            // handle that collided with it would invite exactly that.
            next: 1,
            stale_floor: 0,
            frames: Side::default(),
            variables: Side::default(),
        }
    }
}

impl HandleTable {
    /// Forget the previous stop's handles, once, when the program has moved.
    ///
    /// Lazy rather than driven by the state writer: the table is only
    /// interesting to a request, and a request always brings the current
    /// generation with it. `next` is deliberately *not* reset — reusing a
    /// number is the failure mode this module exists to remove.
    fn sync(&mut self, generation: u64) {
        if self.generation == generation {
            return;
        }
        self.generation = generation;
        self.stale_floor = self.next - 1;
        self.frames.clear();
        self.variables.clear();
    }

    fn side(&mut self, kind: HandleKind) -> &mut Side {
        match kind {
            HandleKind::Frame => &mut self.frames,
            HandleKind::Variables => &mut self.variables,
        }
    }

    /// Our handle for one of the adapter's numbers at this stop.
    ///
    /// The same adapter number asked about twice at the same stop yields the
    /// same handle. That is not an optimisation: a client that fetched a
    /// frame's scopes twice and got two different references for one scope
    /// would have no way to tell they were the same thing.
    pub fn mint(&mut self, generation: u64, kind: HandleKind, adapter: i64) -> i64 {
        self.sync(generation);
        let next = self.next;
        let side = self.side(kind);
        if let Some(existing) = side.to_ours.get(&adapter) {
            return *existing;
        }
        side.to_adapter.insert(next, adapter);
        side.to_ours.insert(adapter, next);
        self.next += 1;
        next
    }

    /// The adapter's number for a handle a caller presented, or why not.
    pub fn resolve(
        &mut self,
        generation: u64,
        kind: HandleKind,
        handle: i64,
    ) -> Result<i64, IpcError> {
        self.sync(generation);
        if let Some(adapter) = self.side(kind).to_adapter.get(&handle) {
            return Ok(*adapter);
        }

        let stale = handle > 0 && handle <= self.stale_floor;
        Err(if stale {
            IpcError::new(
                ErrorCode::StaleHandle,
                format!(
                    "{} {handle} belongs to an earlier stop; the program has moved since. \
                     Ask {} again for this one",
                    kind.noun(),
                    kind.source(),
                ),
            )
        } else {
            IpcError::new(
                ErrorCode::BadRequest,
                format!(
                    "no {} {handle} at this stop; they come from {}",
                    kind.noun(),
                    kind.source(),
                ),
            )
        }
        .with_details(serde_json::json!({
            "handle": handle,
            "kind": kind.noun(),
            "stale": stale,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handle_minted_at_this_stop_resolves_to_the_adapter_s_own_number() {
        let mut table = HandleTable::default();
        let handle = table.mint(4, HandleKind::Variables, 1007);

        assert_eq!(table.resolve(4, HandleKind::Variables, handle), Ok(1007));
    }

    #[test]
    fn no_handle_is_ever_zero_because_zero_already_means_something() {
        // A `variables_reference` of 0 is DAP's "this is a scalar". A handle
        // that collided with it would invite a client to try expanding one.
        let mut table = HandleTable::default();
        assert_ne!(table.mint(1, HandleKind::Variables, 99), 0);
    }

    #[test]
    fn one_adapter_number_asked_about_twice_at_one_stop_is_one_handle() {
        let mut table = HandleTable::default();
        let first = table.mint(2, HandleKind::Frame, 1000);
        let second = table.mint(2, HandleKind::Frame, 1000);

        assert_eq!(first, second, "two names for one thing is not an answer");
    }

    #[test]
    fn a_handle_from_an_earlier_stop_is_refused_as_stale() {
        let mut table = HandleTable::default();
        let handle = table.mint(2, HandleKind::Variables, 1007);

        let error = table
            .resolve(3, HandleKind::Variables, handle)
            .expect_err("the program has moved");

        assert_eq!(error.code, ErrorCode::StaleHandle, "got: {error}");
        assert_eq!(error.details["stale"], true);
    }

    /// The finding this module exists for, and the one a check against the
    /// adapter's own numbers cannot catch.
    ///
    /// The adapter recycles `1007` at the next stop for something else. Under
    /// the old scheme the caller's remembered `1007` sailed through and came
    /// back full of another frame's variables, exit 0. Handles are never
    /// reused, so the stale one and the fresh one are simply different numbers
    /// and only the fresh one resolves.
    #[test]
    fn a_stale_handle_is_still_refused_when_the_adapter_has_recycled_its_number() {
        let mut table = HandleTable::default();
        let stale = table.mint(2, HandleKind::Variables, 1007);

        // Next stop. codelldb hands `1007` out again, for a different scope.
        let fresh = table.mint(3, HandleKind::Variables, 1007);

        assert_ne!(stale, fresh, "a recycled adapter number is not a recycled handle");
        assert_eq!(
            table
                .resolve(3, HandleKind::Variables, stale)
                .expect_err("stale")
                .code,
            ErrorCode::StaleHandle,
        );
        assert_eq!(table.resolve(3, HandleKind::Variables, fresh), Ok(1007));
    }

    #[test]
    fn a_number_that_was_never_a_handle_is_a_bad_request_not_a_stale_one() {
        // `--frame 0` is the case: an obvious thing to type, never issued by
        // anything, and nothing to do with the program having moved. Telling
        // the caller it went stale would send them to re-run `stack` when the
        // real fix is to stop making the number up.
        let mut table = HandleTable::default();
        table.mint(1, HandleKind::Frame, 1000);

        let error = table
            .resolve(1, HandleKind::Frame, 0)
            .expect_err("never issued");

        assert_eq!(error.code, ErrorCode::BadRequest, "got: {error}");
        assert!(error.to_string().contains("lazydap stack"), "got: {error}");
    }

    #[test]
    fn a_frame_handle_is_not_a_variables_handle() {
        let mut table = HandleTable::default();
        let frame = table.mint(1, HandleKind::Frame, 1000);

        table
            .resolve(1, HandleKind::Variables, frame)
            .expect_err("the kinds are not interchangeable");
    }
}
