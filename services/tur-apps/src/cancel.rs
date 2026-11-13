// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Cancelling an installation that is under way.
//!
//! With `install --events`, the program that runs `tur-apps` (the
//! application centre) may write the line `cancel` to its standard input.
//! Nothing else is read there, and the end of the input means nothing: a
//! caller that gives no input, or closes it, cancels nothing.
//!
//! A cancel is honoured only where stopping is safe and can be taken back
//! (desktop-experience.md, 8.5: "Cancelling is offered while it is safe and
//! says what it leaves behind"):
//!
//! - before anything is changed, after the plan;
//! - while a pinned installer downloads (the partial file and its private
//!   directory are deleted);
//! - after the download and its check, before `tur-runtime` starts;
//! - while `flatpak install` runs: flatpak is stopped, and what it had added
//!   is uninstalled again, as after a failure.
//!
//! Once `tur-runtime app install` has started, or the installation is being
//! recorded, the request is no longer honoured and the installation
//! finishes. A cancelled installation ends with TUR-DESKTOP-APPS-013
//! ("Nothing was done without your confirmation"), the message saying what
//! was taken back.

use std::io::BufRead;
use std::sync::atomic::{AtomicBool, Ordering};

static REQUESTED: AtomicBool = AtomicBool::new(false);

/// The line that asks to cancel.
pub const LINE: &str = "cancel";

/// Whether a cancel has been asked for.
pub fn requested() -> bool {
    REQUESTED.load(Ordering::SeqCst)
}

/// Asks to cancel. For the reader of standard input, and for tests.
pub fn request() {
    REQUESTED.store(true, Ordering::SeqCst);
}

/// Reads standard input on a thread of its own, and asks to cancel when the
/// line `cancel` arrives.
pub fn listen_on_stdin() {
    std::thread::spawn(|| {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(line) if line.trim() == LINE => {
                    request();
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });
}

/// The failure a cancelled installation ends with. `taken_back` says what
/// was undone, in a sentence.
pub fn cancelled(taken_back: &str) -> crate::codes::Failure {
    let mut message = "Cancelled. Nothing was installed.".to_owned();
    if !taken_back.is_empty() {
        message.push(' ');
        message.push_str(taken_back);
    }
    crate::codes::Failure::new(crate::codes::Code::ConfirmationNeeded, message)
}
