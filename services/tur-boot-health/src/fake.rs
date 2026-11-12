// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! A machine in memory, for the tests.
//!
//! [`FakeHost`] has two deployments in boot order, GRUB's environment block,
//! the state directory and `/run`. [`FakeHost::grub`] does what the GRUB
//! fragment of design section 5.3 does, [`FakeHost::finalize`] does what
//! OSTree does at shutdown, and the power can be cut after any number of
//! writes: the write that would come next does not happen, nor any after it.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use crate::host::{Entry, Host, Layout, Session};
use crate::model::{BootcStatus, Deployment, EnvBlock, VAR_COUNTER, VAR_TRIAL_ARGS};

/// A change to a unit's property at a point in time, for the gate tests.
pub(crate) struct At {
    pub(crate) uptime_ms: u64,
    pub(crate) unit: &'static str,
    pub(crate) property: &'static str,
    pub(crate) value: &'static str,
}

pub(crate) struct FakeHost {
    pub(crate) cmdline: String,
    pub(crate) boots: u32,
    /// `None` is a block GRUB and `grub2-editenv` cannot read.
    pub(crate) env: Option<EnvBlock>,
    /// GRUB can read the block but its `save_env` has no effect.
    pub(crate) grub_cannot_save: bool,
    /// In boot order: the first is the default.
    pub(crate) deployments: Vec<Deployment>,
    /// Index of the running deployment.
    pub(crate) booted: usize,
    pub(crate) staged: Option<Deployment>,
    pub(crate) finalization_locked: bool,
    /// `bootc rollback` ran in this boot.
    pub(crate) links_swapped: bool,
    pub(crate) state: BTreeMap<String, String>,
    pub(crate) run: BTreeMap<String, String>,
    /// Every write, in order.
    pub(crate) ops: Vec<String>,
    /// Writes left before the power fails.
    pub(crate) power: Option<usize>,
    pub(crate) dead: bool,
    pub(crate) reboot_requested: bool,
    pub(crate) layout: Layout,
    pub(crate) rollback_fails: bool,
    pub(crate) env_write_fails: bool,
    pub(crate) bootc_fails: bool,
    pub(crate) uptime_ms: u64,
    pub(crate) units: BTreeMap<String, BTreeMap<String, String>>,
    pub(crate) timeline: Vec<At>,
    pub(crate) session: Option<Session>,
    pub(crate) compositor: Option<u32>,
    pub(crate) root: Option<(String, String)>,
    pub(crate) files: BTreeMap<String, String>,
    pub(crate) unreadable: BTreeSet<String>,
    pub(crate) var_writable: bool,
    pub(crate) check_status: BTreeMap<String, Result<i32, String>>,
    pub(crate) failed: Vec<String>,
    pub(crate) watchdog: Option<String>,
    pub(crate) journal: Vec<Entry>,
    pub(crate) console: Vec<String>,
    pub(crate) journal_boot_ids: Vec<String>,
    pub(crate) exported: Vec<String>,
}

pub(crate) fn deployment(checksum: &str, version: &str) -> Deployment {
    Deployment {
        image: "localhost/tur:test".to_owned(),
        transport: "registry".to_owned(),
        digest: format!("sha256:{checksum}"),
        version: version.to_owned(),
        checksum: checksum.to_owned(),
        serial: 0,
        stateroot: "default".to_owned(),
        download_only: false,
        build_id: Some(format!("build-{checksum}")),
    }
}

impl FakeHost {
    /// A healthy machine running one deployment, `old`, after an ordinary
    /// boot to the login screen.
    pub(crate) fn new() -> FakeHost {
        let mut host = FakeHost {
            cmdline: String::new(),
            boots: 0,
            env: Some(EnvBlock::new()),
            grub_cannot_save: false,
            deployments: vec![deployment("old", "1.0")],
            booted: 0,
            staged: None,
            finalization_locked: false,
            links_swapped: false,
            state: BTreeMap::new(),
            run: BTreeMap::new(),
            ops: Vec::new(),
            power: None,
            dead: false,
            reboot_requested: false,
            layout: Layout {
                fstype: "ext4".to_owned(),
                source: "/dev/vda3".to_owned(),
                plain_partition: true,
            },
            rollback_fails: false,
            env_write_fails: false,
            bootc_fails: false,
            uptime_ms: 5_000,
            units: BTreeMap::new(),
            timeline: Vec::new(),
            session: Some(Session {
                id: "2".to_owned(),
                kind: "wayland".to_owned(),
                class: "greeter".to_owned(),
                state: "active".to_owned(),
                uid: 970,
            }),
            compositor: Some(1234),
            root: Some(("overlay".to_owned(), "composefs".to_owned())),
            files: BTreeMap::new(),
            unreadable: BTreeSet::new(),
            var_writable: true,
            check_status: BTreeMap::new(),
            failed: Vec::new(),
            watchdog: Some("iTCO_wdt".to_owned()),
            journal: Vec::new(),
            console: Vec::new(),
            journal_boot_ids: Vec::new(),
            exported: Vec::new(),
        };
        for unit in [
            "dbus.service",
            "systemd-logind.service",
            "systemd-journald.service",
            "systemd-udevd.service",
            "display-manager.service",
        ] {
            host.set_unit(unit, "ActiveState", "active");
        }
        host.set_unit("default.target", "Id", "graphical.target");
        host.set_unit("default.target", "ActiveState", "active");
        host.set_unit("display-manager.service", "NRestarts", "0");
        host.set_unit("ostree-boot-complete.service", "ActiveState", "inactive");
        host.grub();
        host
    }

    pub(crate) fn set_unit(&mut self, unit: &str, property: &str, value: &str) {
        self.units
            .entry(unit.to_owned())
            .or_default()
            .insert(property.to_owned(), value.to_owned());
    }

    pub(crate) fn counter(&self) -> Option<String> {
        self.env
            .as_ref()
            .and_then(|env| env.get(VAR_COUNTER).cloned())
    }

    pub(crate) fn booted_checksum(&self) -> String {
        self.deployments[self.booted].checksum.clone()
    }

    pub(crate) fn default_checksum(&self) -> String {
        self.deployments[0].checksum.clone()
    }

    /// Stages a new deployment beside the running one, as `bootc switch` does.
    pub(crate) fn stage(&mut self, checksum: &str, version: &str) {
        self.staged = Some(deployment(checksum, version));
    }

    /// What OSTree does at shutdown: the staged deployment becomes the
    /// default, and the one that was running stays as the second.
    pub(crate) fn finalize(&mut self) {
        if self.finalization_locked {
            self.staged = None;
            return;
        }
        if let Some(staged) = self.staged.take() {
            let running = self.deployments[self.booted].clone();
            self.deployments = vec![staged, running];
            self.booted = 1;
        }
    }

    /// A restart: the machine goes down and GRUB runs its fragment
    /// (design section 5.3), then starts a kernel.
    pub(crate) fn grub(&mut self) {
        self.dead = false;
        self.power = None;
        self.reboot_requested = false;
        self.links_swapped = false;
        self.run.clear();
        self.boots += 1;
        self.uptime_ms = 5_000;
        let mut arguments = "tur.boot_contract=1".to_owned();
        let mut entry = 0;
        if let Some(env) = self.env.clone()
            && let Some(counter) = env.get(VAR_COUNTER)
            && !counter.is_empty()
        {
            let saved = if counter == "0" || counter == "-1" {
                entry = 1;
                arguments = "tur.boot_contract=1 tur.boot=fallback".to_owned();
                "-1".to_owned()
            } else {
                let left = counter.parse::<i64>().unwrap_or(0) - 1;
                let trial = env.get(VAR_TRIAL_ARGS).cloned().unwrap_or_default();
                arguments = format!(
                    "tur.boot_contract=1 tur.boot=trial tur.boot_tries_left={left} {trial}"
                );
                left.to_string()
            };
            if !self.grub_cannot_save
                && let Some(block) = self.env.as_mut()
            {
                block.insert(VAR_COUNTER.to_owned(), saved);
            }
        }
        self.booted = if entry < self.deployments.len() {
            entry
        } else {
            0
        };
        self.cmdline = format!(
            "BOOT_IMAGE=/vmlinuz root=UUID=1 rw {arguments} ostree=/ostree/boot.1/default/x/0"
        );
    }

    /// GRUB could not start the first entry and started the second in the
    /// same boot (`fallback=1`): the arguments stay those of the first.
    pub(crate) fn loader_fell_back(&mut self) {
        self.booted = 1;
    }

    /// The gate's verdict for this boot, as the gate would leave it.
    pub(crate) fn set_verdict(&mut self, result: &str, failed_check: Option<&str>) {
        let code = match failed_check {
            Some(_) => "\"TUR-BOOT-HEALTH-005\"",
            None => "null",
        };
        let check = match failed_check {
            Some(check) => format!("\"{check}\""),
            None => "null".to_owned(),
        };
        self.run.insert(
            crate::trial::RUN_VERDICT.to_owned(),
            format!(
                "{{\"schema_version\":1,\"boot_id\":\"{}\",\"result\":\"{result}\",\
                 \"failed_check\":{check},\"error_code\":{code},\"failed_detail\":\"it failed\"}}",
                self.boot_id()
            ),
        );
    }

    fn write(&mut self, what: String) -> Result<(), String> {
        if self.dead {
            return Err("the power is off".to_owned());
        }
        if let Some(left) = self.power {
            if left == 0 {
                self.dead = true;
                return Err("the power failed".to_owned());
            }
            self.power = Some(left - 1);
        }
        self.ops.push(what);
        Ok(())
    }

    /// The writes that match a prefix, in order.
    pub(crate) fn ops_matching(&self, prefix: &str) -> Vec<String> {
        self.ops
            .iter()
            .filter(|op| op.starts_with(prefix))
            .cloned()
            .collect()
    }

    pub(crate) fn op_index(&self, prefix: &str) -> usize {
        self.ops
            .iter()
            .position(|op| op.starts_with(prefix))
            .unwrap_or_else(|| panic!("no write starting with {prefix:?} in {:?}", self.ops))
    }
}

impl Host for FakeHost {
    fn cmdline(&self) -> String {
        self.cmdline.clone()
    }

    fn boot_id(&self) -> String {
        format!("boot{:04}", self.boots)
    }

    fn build_id(&self) -> String {
        self.deployments[self.booted]
            .build_id
            .clone()
            .unwrap_or_default()
    }

    fn now_unix(&self) -> i64 {
        1_800_000_000 + i64::from(self.boots)
    }

    fn uptime_ms(&self) -> u64 {
        self.uptime_ms
    }

    fn sleep_ms(&mut self, milliseconds: u64) {
        self.uptime_ms += milliseconds;
        let now = self.uptime_ms;
        let due: Vec<(String, String, String)> = self
            .timeline
            .iter()
            .filter(|at| at.uptime_ms <= now)
            .map(|at| {
                (
                    at.unit.to_owned(),
                    at.property.to_owned(),
                    at.value.to_owned(),
                )
            })
            .collect();
        for (unit, property, value) in due {
            self.set_unit(&unit, &property, &value);
        }
    }

    fn env_list(&mut self) -> Result<EnvBlock, String> {
        self.env
            .clone()
            .ok_or_else(|| "invalid environment block".to_owned())
    }

    fn env_set(&mut self, variables: &[(&str, &str)]) -> Result<(), String> {
        if self.env_write_fails {
            return Err("read-only file system".to_owned());
        }
        let described: Vec<String> = variables
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect();
        self.write(format!("env_set {}", described.join(" ")))?;
        let Some(env) = self.env.as_mut() else {
            return Err("invalid environment block".to_owned());
        };
        for (name, value) in variables {
            env.insert((*name).to_owned(), (*value).to_owned());
        }
        Ok(())
    }

    fn env_unset(&mut self, names: &[&str]) -> Result<(), String> {
        if self.env_write_fails {
            return Err("read-only file system".to_owned());
        }
        self.write(format!("env_unset {}", names.join(" ")))?;
        let Some(env) = self.env.as_mut() else {
            return Err("invalid environment block".to_owned());
        };
        for name in names {
            env.remove(*name);
        }
        Ok(())
    }

    fn env_create(&mut self) -> Result<(), String> {
        self.write("env_create".to_owned())?;
        self.env = Some(EnvBlock::new());
        Ok(())
    }

    fn bootc_status(&mut self) -> Result<BootcStatus, String> {
        if self.bootc_fails {
            return Err("bootc status failed".to_owned());
        }
        Ok(BootcStatus {
            booted: self.deployments.get(self.booted).cloned(),
            staged: self.staged.clone(),
            rollback: if self.deployments.len() > 1 {
                self.deployments.get(1 - self.booted).cloned()
            } else {
                None
            },
            rollback_queued: self.booted != 0,
        })
    }

    fn booted_is_default(&mut self) -> Result<bool, String> {
        Ok(self.booted == 0)
    }

    fn rollback(&mut self) -> Result<(), String> {
        if self.rollback_fails {
            return Err("bootc rollback: error".to_owned());
        }
        if self.deployments.len() < 2 {
            return Err("no rollback deployment".to_owned());
        }
        self.write("rollback".to_owned())?;
        self.links_swapped = true;
        self.deployments.swap(0, 1);
        self.booted = 1 - self.booted;
        Ok(())
    }

    fn lock_finalization(&mut self) -> Result<(), String> {
        self.write("lock_finalization".to_owned())?;
        self.finalization_locked = true;
        Ok(())
    }

    fn layout(&mut self) -> Result<Layout, String> {
        Ok(self.layout.clone())
    }

    fn read_state(&self, name: &str) -> Option<String> {
        self.state.get(name).cloned()
    }

    fn write_state(&mut self, name: &str, content: &str) -> Result<(), String> {
        self.write(format!("write_state {name}"))?;
        self.state.insert(name.to_owned(), content.to_owned());
        Ok(())
    }

    fn remove_state(&mut self, name: &str) -> Result<(), String> {
        self.write(format!("remove_state {name}"))?;
        self.state.remove(name);
        Ok(())
    }

    fn remove_state_tree(&mut self, name: &str) -> Result<(), String> {
        self.write(format!("remove_state_tree {name}"))?;
        let prefix = format!("{name}/");
        self.state.retain(|key, _| !key.starts_with(&prefix));
        Ok(())
    }

    fn read_run(&self, name: &str) -> Option<String> {
        self.run.get(name).cloned()
    }

    fn write_run(&mut self, name: &str, content: &str) -> Result<(), String> {
        if !self.dead {
            self.run.insert(name.to_owned(), content.to_owned());
        }
        Ok(())
    }

    fn journal(&mut self, entry: &Entry) {
        if !self.dead {
            self.journal.push(entry.clone());
        }
    }

    fn sync_journal(&mut self) {}

    fn console(&mut self, line: &str) {
        if !self.dead {
            self.console.push(line.to_owned());
        }
    }

    fn reboot(&mut self) -> Result<(), String> {
        self.reboot_requested = true;
        Ok(())
    }

    fn unit_show(
        &mut self,
        unit: &str,
        properties: &[&str],
    ) -> Result<BTreeMap<String, String>, String> {
        let known = self.units.get(unit).cloned().unwrap_or_default();
        let mut found = BTreeMap::new();
        for property in properties {
            let value = match (known.get(*property), *property) {
                (Some(value), _) => value.clone(),
                (None, "Id") => unit.to_owned(),
                (None, "ActiveState") => "inactive".to_owned(),
                (None, _) => String::new(),
            };
            found.insert((*property).to_owned(), value);
        }
        Ok(found)
    }

    fn seat_session(&mut self) -> Result<Option<Session>, String> {
        Ok(self.session.clone())
    }

    fn compositor_pid(&mut self, _uid: u32) -> Option<u32> {
        self.compositor
    }

    fn root_mount(&mut self) -> Option<(String, String)> {
        self.root.clone()
    }

    fn ostree_target(&mut self, _ostree_argument: &str) -> Option<String> {
        // After a rollback in this boot the links the argument goes through
        // lead to the other deployment (seen in CI, run 43200240515).
        let index = if self.links_swapped && self.deployments.len() > 1 {
            1 - self.booted
        } else {
            self.booted
        };
        self.deployments.get(index).map(Deployment::name)
    }

    fn path_readable(&mut self, path: &str) -> bool {
        !self.unreadable.contains(path)
    }

    fn dir_writable(&mut self, _path: &str) -> bool {
        self.var_writable
    }

    fn list_dir(&mut self, path: &str) -> Vec<String> {
        let prefix = format!("{path}/");
        let mut names: BTreeSet<String> = BTreeSet::new();
        for key in self.files.keys() {
            if let Some(rest) = key.strip_prefix(&prefix) {
                names.insert(rest.split('/').next().unwrap_or(rest).to_owned());
            }
        }
        if let Some(rest) = path.strip_prefix("/var/lib/tur/boot-health/") {
            let state_prefix = format!("{rest}/");
            for key in self.state.keys() {
                if let Some(rest) = key.strip_prefix(&state_prefix) {
                    names.insert(rest.split('/').next().unwrap_or(rest).to_owned());
                }
            }
        }
        names.into_iter().collect()
    }

    fn read_file(&mut self, path: &str) -> Option<String> {
        self.files.get(path).cloned()
    }

    fn run_check(&mut self, exec: &str, _args: &[String], _timeout_s: u64) -> Result<i32, String> {
        self.check_status
            .get(exec)
            .cloned()
            .unwrap_or(Err(format!("{exec}: No such file or directory")))
    }

    fn failed_units(&mut self) -> Vec<String> {
        self.failed.clone()
    }

    fn watchdog(&mut self) -> Option<String> {
        self.watchdog.clone()
    }

    fn journal_boots(&mut self) -> Vec<String> {
        self.journal_boot_ids.clone()
    }

    fn export_journal(&mut self, boot_id: &str, name: &str, _limit: u64) -> Result<String, String> {
        if !self.journal_boot_ids.iter().any(|known| known == boot_id) {
            return Err(format!("the journal has nothing for boot {boot_id}"));
        }
        self.write(format!("export_journal {name}"))?;
        self.exported.push(boot_id.to_owned());
        let file = format!("{name}.zst");
        self.state.insert(file.clone(), "journal".to_owned());
        Ok(file)
    }
}
