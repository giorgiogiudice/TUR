// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The only door to the machine.
//!
//! Everything this program reads or changes goes through [`Host`]: GRUB's
//! environment block, `bootc` and `ostree`, the state files, the journal,
//! systemd and logind. The logic in the other modules is written against the
//! trait, and the tests replace it with a machine in memory
//! (`crate::fake::FakeHost`) that can also lose power at any write.
//!
//! The real implementation is [`crate::real::SystemHost`]. It talks to other
//! software through command lines only, each with a time limit and no shell
//! (design section 5.1).

use std::collections::BTreeMap;

use crate::model::{BootcStatus, EnvBlock};

/// One journal entry: fields in the journal's native format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    /// The fixed message ID of this kind of entry.
    pub(crate) message_id: &'static str,
    /// syslog priority, 0 to 7.
    pub(crate) priority: u8,
    pub(crate) message: String,
    /// Further fields, upper-case names.
    pub(crate) fields: Vec<(String, String)>,
}

/// The session that owns the seat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Session {
    pub(crate) id: String,
    /// logind's `Type`: `wayland`, `x11`, `tty`, …
    pub(crate) kind: String,
    /// logind's `Class`: `greeter`, `user`, …
    pub(crate) class: String,
    /// logind's `State`: `active`, `online`, `closing`.
    pub(crate) state: String,
    pub(crate) uid: u32,
}

/// The filesystem that holds `/boot/grub2`, where GRUB must be able to write
/// its environment block in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Layout {
    pub(crate) fstype: String,
    pub(crate) source: String,
    /// The source is a partition of a disk: not LVM, RAID or a mapped device.
    pub(crate) plain_partition: bool,
}

impl Layout {
    /// The layouts on which counting has been shown to work in CI. The list
    /// grows only with a CI test of the new layout (design section 5.5).
    pub(crate) fn counting_supported(&self) -> bool {
        self.fstype == "ext4" && self.plain_partition
    }

    pub(crate) fn describe(&self) -> String {
        format!(
            "{} on {} ({})",
            self.fstype,
            self.source,
            if self.plain_partition {
                "a plain partition"
            } else {
                "not a plain partition"
            }
        )
    }
}

/// The machine, as far as this program is concerned.
pub(crate) trait Host {
    // ---- this boot ----------------------------------------------------------

    /// `/proc/cmdline`.
    fn cmdline(&self) -> String;
    /// The boot ID, 32 hexadecimal digits as the journal writes it.
    fn boot_id(&self) -> String;
    /// `TUR_BUILD_ID` of the running image, or an empty string.
    fn build_id(&self) -> String;
    /// Wall-clock seconds. Recorded for people; no decision depends on it.
    fn now_unix(&self) -> i64;
    /// Milliseconds since the kernel started.
    fn uptime_ms(&self) -> u64;
    /// Waits.
    fn sleep_ms(&mut self, milliseconds: u64);

    // ---- GRUB's environment block ---------------------------------------------

    /// The variables in the block. An error means the block is missing or
    /// not a valid block.
    fn env_list(&mut self) -> Result<EnvBlock, String>;
    /// Sets variables in one write, then syncs.
    fn env_set(&mut self, variables: &[(&str, &str)]) -> Result<(), String>;
    /// Removes variables in one write, then syncs.
    fn env_unset(&mut self, names: &[&str]) -> Result<(), String>;
    /// Replaces the block with a valid, empty one.
    fn env_create(&mut self) -> Result<(), String>;

    // ---- deployments ---------------------------------------------------------------

    /// `bootc status --format=json`.
    fn bootc_status(&mut self) -> Result<BootcStatus, String>;
    /// `ostree admin status --is-default`: is the booted deployment the one
    /// the bootloader starts by default?
    fn booted_is_default(&mut self) -> Result<bool, String>;
    /// `bootc rollback`.
    fn rollback(&mut self) -> Result<(), String>;
    /// `ostree admin lock-finalization`: do not apply the staged deployment
    /// at shutdown.
    fn lock_finalization(&mut self) -> Result<(), String>;
    /// The filesystem under `/boot/grub2`.
    fn layout(&mut self) -> Result<Layout, String>;

    // ---- state under /var/lib/tur/boot-health and /run/tur ----------------------------

    fn read_state(&self, name: &str) -> Option<String>;
    /// Writes a state file so that it is either the old or the new content,
    /// and is on disk when this returns.
    fn write_state(&mut self, name: &str, content: &str) -> Result<(), String>;
    fn remove_state(&mut self, name: &str) -> Result<(), String>;
    /// Removes a directory under the state directory with everything in it.
    fn remove_state_tree(&mut self, name: &str) -> Result<(), String>;
    fn read_run(&self, name: &str) -> Option<String>;
    fn write_run(&mut self, name: &str, content: &str) -> Result<(), String>;

    // ---- reporting --------------------------------------------------------------------

    /// Writes a journal entry.
    fn journal(&mut self, entry: &Entry);
    /// `journalctl --sync`.
    fn sync_journal(&mut self);
    /// A line for the console and the journal. The units that matter send
    /// their output to both, so the line is visible on a screen and to the
    /// test harness even when nothing else is.
    fn console(&mut self, line: &str);
    /// Asks systemd to restart the machine.
    fn reboot(&mut self) -> Result<(), String>;

    // ---- what the gate looks at ----------------------------------------------------------

    /// Properties of a unit, from `systemctl show`.
    fn unit_show(
        &mut self,
        unit: &str,
        properties: &[&str],
    ) -> Result<BTreeMap<String, String>, String>;
    /// The active session on `seat0`, if there is one.
    fn seat_session(&mut self) -> Result<Option<Session>, String>;
    /// The process ID of a compositor owned by a user, if one runs.
    fn compositor_pid(&mut self, uid: u32) -> Option<u32>;
    /// The filesystem type and source of the mount at `/`.
    fn root_mount(&mut self) -> Option<(String, String)>;
    /// The name of the deployment directory the kernel's `ostree=` argument
    /// resolves to, `checksum.serial`.
    fn ostree_target(&mut self, ostree_argument: &str) -> Option<String>;
    fn path_readable(&mut self, path: &str) -> bool;
    fn dir_writable(&mut self, path: &str) -> bool;
    /// The entries of a directory, sorted. Empty if it cannot be read.
    fn list_dir(&mut self, path: &str) -> Vec<String>;
    fn read_file(&mut self, path: &str) -> Option<String>;
    /// Runs an additional check without a shell. `Ok` carries its exit
    /// status; an error means it could not be run or ran out of time.
    fn run_check(&mut self, exec: &str, args: &[String], timeout_s: u64) -> Result<i32, String>;
    /// The names of the units in the failed state.
    fn failed_units(&mut self) -> Vec<String>;
    /// The identity of the first hardware watchdog, if the kernel has one.
    fn watchdog(&mut self) -> Option<String>;

    // ---- evidence -------------------------------------------------------------------------

    /// The boot IDs in the persistent journal, oldest first.
    fn journal_boots(&mut self) -> Vec<String>;
    /// Exports the journal of one boot into the state directory, at most
    /// `limit` bytes of it. Returns the file name written.
    fn export_journal(&mut self, boot_id: &str, name: &str, limit: u64) -> Result<String, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counting_is_supported_on_ext4_on_a_plain_partition_only() {
        let layout = |fstype: &str, plain: bool| Layout {
            fstype: fstype.to_owned(),
            source: "/dev/vda3".to_owned(),
            plain_partition: plain,
        };
        assert!(layout("ext4", true).counting_supported());
        assert!(!layout("ext4", false).counting_supported());
        assert!(!layout("btrfs", true).counting_supported());
        assert!(!layout("xfs", true).counting_supported());
        assert!(layout("ext4", true).describe().contains("plain partition"));
    }
}
