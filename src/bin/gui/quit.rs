//! The quit guard's state machine — the half that is not Objective-C.
//!
//! # Why this file exists
//!
//! On Windows and Linux there is exactly one way to close this window (the
//! title-bar ✕ / Alt-F4), it arrives as winit's `CloseRequested`, and
//! `AutoShadeApp::update` intercepts it: busy → refuse and say so, unsaved →
//! the in-app save-all / discard / cancel layer (`app.rs`, the
//! `close_requested()` branch).
//!
//! macOS has four more ways, and none of them reach that branch. ⌘Q, the
//! App-menu Quit item, the Dock's right-click Quit, and log-out/shutdown all
//! send `-[NSApplication terminate:]`. winit installs the default application
//! menu on every macOS build — `default_menu` is `true` unless the embedder
//! opts out (winit 0.30.13 `platform_impl/macos/event_loop.rs:212`) and the
//! Quit item it builds is wired straight to `terminate:`
//! (`platform_impl/macos/menu.rs:71`) — so this is not something an app can
//! decline by not asking for a menu. `terminate:` tears the process down
//! through `applicationWillTerminate:`, which winit's delegate does implement
//! (`platform_impl/macos/app_state.rs:69`); `windowShouldClose:` is never
//! consulted, so no `CloseRequested` is ever generated and the guard above
//! simply does not run. Every unsaved develop, every stashed photo, and any
//! export in flight would go with it, silently.
//!
//! AppKit's own answer is `applicationShouldTerminate:`: a delegate method
//! that runs BEFORE the teardown and can veto it. winit's delegate does not
//! implement it, and it may not be replaced wholesale (winit panics if the
//! `NSApp` delegate is swapped out from under it). `macos.rs` therefore adds
//! that ONE method to winit's existing delegate class at runtime, and the
//! method's whole body is a call into this module.
//!
//! # The split, and why the state machine lives here
//!
//! Everything below is ordinary Rust with no Apple types in it, so it compiles
//! and is TESTED on every platform — including the Windows battery, which is
//! the one that gates this repo. `macos.rs` holds only the Objective-C runtime
//! call and the two `NSApplicationTerminateReply` constants; it is
//! `cfg(target_os = "macos")` and can be checked, but never run, here.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::OnceLock;

/// What the window-close guard would decide if the window were asked to close
/// at this instant.
///
/// Published every frame by `AutoShadeApp::update` from the SAME predicates the
/// close guard itself uses, so the two can never disagree about what quitting
/// would cost — a second copy of "is there unsaved work" is exactly the drift
/// that would make this guard lie.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum QuitState {
    /// Nothing is running and nothing is unsaved: quitting loses no work.
    #[default]
    Clean,
    /// A job is running, edits are unsaved, or a typed name is uncommitted.
    /// Quitting now would destroy work the user has not been asked about.
    Dirty,
    /// The user has ALREADY been asked this session and answered — save-all
    /// finished, or discard was chosen — and a close is on its way.
    ///
    /// Distinct from `Clean` on purpose. Discard cleans the active canvas but
    /// cannot clean a background variant (its edits have nowhere to be saved),
    /// so the raw "is anything unsaved" inputs stay true forever afterwards.
    /// Collapsing this into `Dirty` would re-ask a question the user just
    /// answered and make ⌘Q unable to ever quit; collapsing it into `Clean`
    /// would claim there was nothing to lose, which is not what happened.
    Confirmed,
}

impl QuitState {
    /// The transition function: the close guard's own inputs, in the close
    /// guard's own order.
    ///
    /// `busy` is checked FIRST and outranks `answered`. A running export or a
    /// paid generation dies with the process, and no answer the user gave to
    /// the unsaved-work question was an answer about that — `app.rs` refuses
    /// the close and says why, which is what `Dirty` routes to here.
    ///
    /// `unsaved` is a CLOSURE because this runs once per frame and the real
    /// predicate behind it walks the variant strip deep-comparing recipes. The
    /// two cheap inputs above decide on their own most of the time, and when
    /// they do the walk must not happen at all.
    pub(crate) fn from_app(busy: bool, answered: bool, unsaved: impl FnOnce() -> bool) -> Self {
        if busy {
            Self::Dirty
        } else if answered {
            Self::Confirmed
        } else if unsaved() {
            Self::Dirty
        } else {
            Self::Clean
        }
    }

    fn as_u8(self) -> u8 {
        match self {
            Self::Clean => 0,
            Self::Dirty => 1,
            Self::Confirmed => 2,
        }
    }

    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Dirty,
            2 => Self::Confirmed,
            _ => Self::Clean,
        }
    }
}

/// What to hand back to `-[NSApplication terminate:]`.
///
/// The numeric `NSApplicationTerminateReply` values live in `macos.rs`; this
/// half of the decision is deliberately unitless so it can be tested anywhere.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TerminateReply {
    /// Let AppKit tear the process down.
    Now,
    /// Veto it. The app was asked to close through its normal path instead, so
    /// the user gets the in-app layer rather than a vanished window.
    Cancel,
}

/// The decision, with no globals in sight.
///
/// The second element is "and ask the app to close": it is true exactly when
/// the reply is `Cancel`, because a veto that did not also start the in-app
/// close would leave ⌘Q doing *nothing at all* — a silent no-op is a worse
/// answer than the data loss it replaced, since the user would keep pressing it.
pub(crate) fn decide(state: QuitState) -> (TerminateReply, bool) {
    match state {
        QuitState::Clean | QuitState::Confirmed => (TerminateReply::Now, false),
        QuitState::Dirty => (TerminateReply::Cancel, true),
    }
}

// --- the process-wide slots the Objective-C method reaches through ----------
//
// A delegate method is called by AppKit with no user data of ours, so the
// bridge to the app is necessarily global. All three are written only by the
// GUI's own main thread and read by it, on the same thread AppKit calls the
// delegate on; the atomics are for the type system's benefit as much as the
// hardware's.

static STATE: AtomicU8 = AtomicU8::new(0);
static CLOSE_REQUESTED: AtomicBool = AtomicBool::new(false);
static CTX: OnceLock<egui::Context> = OnceLock::new();

/// Record what quitting would cost right now. Called once per frame.
pub(crate) fn publish(state: QuitState) {
    STATE.store(state.as_u8(), Ordering::Relaxed);
}

/// The last published state.
pub(crate) fn state() -> QuitState {
    QuitState::from_u8(STATE.load(Ordering::Relaxed))
}

/// Hand the repaint handle to the guard.
///
/// Vetoing a terminate returns control to an AppKit run loop that may then sit
/// idle: egui is a lazy repainter, so without this the confirm layer would not
/// appear until the user happened to move the mouse. Set once, from the app's
/// creation closure.
pub(crate) fn set_ctx(ctx: &egui::Context) {
    let _ = CTX.set(ctx.clone());
}

/// The body of `applicationShouldTerminate:`, minus Objective-C.
///
/// Returns what to tell AppKit. On a veto it also arms [`take_close_request`]
/// and wakes the UI, so the next frame runs the ordinary close path — the
/// SAME guard the title-bar ✕ goes through, rather than a second dialog that
/// would have to be kept in step with it.
pub(crate) fn on_terminate_request() -> TerminateReply {
    let (reply, ask_close) = decide(state());
    if ask_close {
        CLOSE_REQUESTED.store(true, Ordering::Relaxed);
        if let Some(ctx) = CTX.get() {
            ctx.request_repaint();
        }
    }
    reply
}

/// Take a pending "the user asked to quit" flag, clearing it.
pub(crate) fn take_close_request() -> bool {
    CLOSE_REQUESTED.swap(false, Ordering::Relaxed)
}
