// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! What the assistant can propose to do.
//!
//! An [`Action`] is a proposal. It carries no authority: the policy engine
//! decides, from the action, the context and the user policy, whether it runs.

use serde_json::{Map, Value};

use crate::class::ActionClass;

/// One proposed action.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Action {
    /// A typed tool call on a UOS service (tier 1).
    Tool(ToolCall),
    /// An action through the accessibility tree (tier 2).
    Accessibility(UiAction),
    /// A screen capture or emulated input through the compositor (tier 3).
    ScreenInput(UiAction),
    /// A command run in a sandbox (the fourth way of acting).
    Command(CommandAction),
    /// One of the agent's own built-in operations, such as writing to memory.
    Agent(AgentOperation),
}

impl Action {
    /// The tier recorded in the audit log.
    ///
    /// 1 to 3 are the tiers of the addendum; 4 is a command; 0 is an operation
    /// of the agent itself, which uses none of the ways of acting.
    pub const fn tier(&self) -> u8 {
        match self {
            Action::Agent(_) => 0,
            Action::Tool(_) => 1,
            Action::Accessibility(_) => 2,
            Action::ScreenInput(_) => 3,
            Action::Command(_) => 4,
        }
    }
}

/// A call to a tool declared in a service's manifest.
///
/// The only way to build one outside this crate is
/// [`crate::ToolManifest::tool_call`]. The service, the tool name and the
/// declared class therefore always come from a validated manifest, never from
/// the model, and the arguments have been checked against the input schema.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub(crate) service: String,
    pub(crate) tool: String,
    pub(crate) arguments: Map<String, Value>,
    pub(crate) declared_class: ActionClass,
}

impl ToolCall {
    /// The D-Bus name of the service that owns the tool.
    pub fn service(&self) -> &str {
        &self.service
    }

    /// The tool name.
    pub fn tool(&self) -> &str {
        &self.tool
    }

    /// The arguments proposed by the model. They are data and carry no authority.
    pub fn arguments(&self) -> &Map<String, Value> {
        &self.arguments
    }

    /// The class the service declared for this tool in its manifest.
    pub fn declared_class(&self) -> ActionClass {
        self.declared_class
    }
}

/// Whether a UI action only reads or also acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiActionKind {
    /// Reads the accessibility tree or captures the screen.
    Observe,
    /// Activates a control, types, or moves the pointer.
    Act,
}

impl UiActionKind {
    /// Both kinds.
    pub const ALL: [UiActionKind; 2] = [UiActionKind::Observe, UiActionKind::Act];

    /// The kind as text, for the audit log.
    pub const fn as_str(self) -> &'static str {
        match self {
            UiActionKind::Observe => "observe",
            UiActionKind::Act => "act",
        }
    }
}

/// An accessibility or screen-and-input action.
///
/// The application and window title must come from the compositor or the
/// accessibility bus, not from the model. See the known limits in
/// `docs/architecture/agent-policy.md` for the control description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiAction {
    /// Identifier of the target application.
    pub application: String,
    /// Title of the target window; empty if unknown.
    pub window_title: String,
    /// Description of the control acted on, such as its accessible name.
    pub control: String,
    /// Whether the action only reads or also acts.
    pub kind: UiActionKind,
}

/// A command and the sandbox it would run in.
///
/// A command is never classified by parsing its text. Its class comes from the
/// sandbox alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandAction {
    /// The sandbox the command would run in.
    pub sandbox: Sandbox,
    /// The program and its arguments. Shown to the user exactly; never parsed.
    pub argv: Vec<String>,
    /// Where the command's changes land.
    pub working_context: WorkingContext,
}

/// The sandbox a command runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sandbox {
    /// A read-only view of the system with no write access anywhere that persists.
    ReadOnly,
    /// The user's own privileges; can change the user's files.
    User,
    /// Root, through the privileged helper, for one typed kind of operation.
    Privileged(PrivilegedOperation),
}

impl Sandbox {
    /// The sandbox kind as text, for the audit log.
    pub const fn as_str(self) -> &'static str {
        match self {
            Sandbox::ReadOnly => "read_only",
            Sandbox::User => "user",
            Sandbox::Privileged(_) => "privileged",
        }
    }
}

/// Where a command's changes land.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkingContext {
    /// The user's session and files, with no automatic undo.
    Session,
    /// A task workspace whose changes the system can undo.
    UndoableWorkspace,
}

impl WorkingContext {
    /// Both working contexts.
    pub const ALL: [WorkingContext; 2] =
        [WorkingContext::Session, WorkingContext::UndoableWorkspace];
}

/// The kind of operation a privileged command asks the privileged helper for.
///
/// The operation type is a typed field of the helper request. It selects the
/// confinement the helper applies; it is not derived from the command text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrivilegedOperation {
    /// Reading privileged logs and state.
    Diagnostics,
    /// Starting, stopping and restarting system services.
    ServiceControl,
    /// Changing system configuration outside the system-critical areas.
    SystemConfiguration,
    /// Changing network configuration.
    NetworkConfiguration,
    /// Installing, removing and repairing software.
    SoftwareManagement,
    /// Creating, deleting or resizing partitions.
    Partitioning,
    /// Creating or wiping a filesystem.
    Formatting,
    /// Setting up, changing or removing disk encryption.
    Encryption,
    /// Changing the bootloader or its configuration.
    Bootloader,
    /// Updating or configuring firmware.
    Firmware,
    /// Updating or rolling back the operating system.
    OsUpdate,
    /// Handing a device to a virtual machine.
    DeviceHandOff,
    /// Changing security policy.
    SecurityPolicy,
    /// An operation the helper cannot assign to any other type.
    Unclassified,
}

impl PrivilegedOperation {
    /// Every operation type.
    pub const ALL: [PrivilegedOperation; 14] = [
        PrivilegedOperation::Diagnostics,
        PrivilegedOperation::ServiceControl,
        PrivilegedOperation::SystemConfiguration,
        PrivilegedOperation::NetworkConfiguration,
        PrivilegedOperation::SoftwareManagement,
        PrivilegedOperation::Partitioning,
        PrivilegedOperation::Formatting,
        PrivilegedOperation::Encryption,
        PrivilegedOperation::Bootloader,
        PrivilegedOperation::Firmware,
        PrivilegedOperation::OsUpdate,
        PrivilegedOperation::DeviceHandOff,
        PrivilegedOperation::SecurityPolicy,
        PrivilegedOperation::Unclassified,
    ];

    /// Whether a repair session may cover this operation type.
    ///
    /// This is the deny list of the repair session. The system-critical
    /// operations are never covered, and neither is an unclassified one. The
    /// match has no wildcard arm, so a new operation type cannot be added
    /// without deciding which side it is on.
    pub const fn repair_session_may_cover(self) -> bool {
        match self {
            PrivilegedOperation::Diagnostics
            | PrivilegedOperation::ServiceControl
            | PrivilegedOperation::SystemConfiguration
            | PrivilegedOperation::NetworkConfiguration
            | PrivilegedOperation::SoftwareManagement => true,
            PrivilegedOperation::Partitioning
            | PrivilegedOperation::Formatting
            | PrivilegedOperation::Encryption
            | PrivilegedOperation::Bootloader
            | PrivilegedOperation::Firmware
            | PrivilegedOperation::OsUpdate
            | PrivilegedOperation::DeviceHandOff
            | PrivilegedOperation::SecurityPolicy
            | PrivilegedOperation::Unclassified => false,
        }
    }

    /// The operation type as text, for prompts and the audit log.
    pub const fn as_str(self) -> &'static str {
        match self {
            PrivilegedOperation::Diagnostics => "diagnostics",
            PrivilegedOperation::ServiceControl => "service_control",
            PrivilegedOperation::SystemConfiguration => "system_configuration",
            PrivilegedOperation::NetworkConfiguration => "network_configuration",
            PrivilegedOperation::SoftwareManagement => "software_management",
            PrivilegedOperation::Partitioning => "partitioning",
            PrivilegedOperation::Formatting => "formatting",
            PrivilegedOperation::Encryption => "encryption",
            PrivilegedOperation::Bootloader => "bootloader",
            PrivilegedOperation::Firmware => "firmware",
            PrivilegedOperation::OsUpdate => "os_update",
            PrivilegedOperation::DeviceHandOff => "device_hand_off",
            PrivilegedOperation::SecurityPolicy => "security_policy",
            PrivilegedOperation::Unclassified => "unclassified",
        }
    }
}

/// A built-in operation of the agent itself.
///
/// These are not service tools, so no manifest declares them. Their classes are
/// fixed here, in [`AgentOperation::class`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentOperation {
    /// Recall from the assistant's memory.
    MemoryRead,
    /// Store, change or remove an entry in the assistant's memory.
    MemoryWrite,
    /// Create, change or remove a reminder or scheduled task.
    ScheduleWrite,
    /// Read a secret from the keyring.
    KeyringSecretRead,
    /// Change the assistant's policy.
    PolicyWrite,
    /// Change the assistant's permissions.
    PermissionWrite,
    /// Create or change a standing approval.
    StandingApprovalWrite,
    /// Disable or hide the activity indicator.
    IndicatorDisable,
    /// Disable or alter the audit log.
    AuditLogDisable,
    /// Operate the trusted confirmation path.
    TrustedPathOperate,
    /// Enable experimental APFS write support.
    ApfsWriteEnable,
}

impl AgentOperation {
    /// Every built-in operation.
    pub const ALL: [AgentOperation; 11] = [
        AgentOperation::MemoryRead,
        AgentOperation::MemoryWrite,
        AgentOperation::ScheduleWrite,
        AgentOperation::KeyringSecretRead,
        AgentOperation::PolicyWrite,
        AgentOperation::PermissionWrite,
        AgentOperation::StandingApprovalWrite,
        AgentOperation::IndicatorDisable,
        AgentOperation::AuditLogDisable,
        AgentOperation::TrustedPathOperate,
        AgentOperation::ApfsWriteEnable,
    ];

    /// The fixed class of the operation.
    pub const fn class(self) -> ActionClass {
        match self {
            AgentOperation::MemoryRead => ActionClass::Observe,
            AgentOperation::MemoryWrite => ActionClass::Reversible,
            AgentOperation::ScheduleWrite => ActionClass::Consequential,
            AgentOperation::KeyringSecretRead
            | AgentOperation::PolicyWrite
            | AgentOperation::PermissionWrite
            | AgentOperation::StandingApprovalWrite
            | AgentOperation::IndicatorDisable
            | AgentOperation::AuditLogDisable
            | AgentOperation::TrustedPathOperate
            | AgentOperation::ApfsWriteEnable => ActionClass::Prohibited,
        }
    }

    /// The operation as text, for prompts and the audit log.
    pub const fn as_str(self) -> &'static str {
        match self {
            AgentOperation::MemoryRead => "memory_read",
            AgentOperation::MemoryWrite => "memory_write",
            AgentOperation::ScheduleWrite => "schedule_write",
            AgentOperation::KeyringSecretRead => "keyring_secret_read",
            AgentOperation::PolicyWrite => "policy_write",
            AgentOperation::PermissionWrite => "permission_write",
            AgentOperation::StandingApprovalWrite => "standing_approval_write",
            AgentOperation::IndicatorDisable => "indicator_disable",
            AgentOperation::AuditLogDisable => "audit_log_disable",
            AgentOperation::TrustedPathOperate => "trusted_path_operate",
            AgentOperation::ApfsWriteEnable => "apfs_write_enable",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_session_never_covers_the_system_critical_operations() {
        use PrivilegedOperation as Op;
        // The list in the revision 2 rules, plus the unclassified type.
        let never = [
            Op::Partitioning,
            Op::Formatting,
            Op::Encryption,
            Op::Bootloader,
            Op::Firmware,
            Op::OsUpdate,
            Op::DeviceHandOff,
            Op::SecurityPolicy,
            Op::Unclassified,
        ];
        for operation in Op::ALL {
            assert_eq!(
                operation.repair_session_may_cover(),
                !never.contains(&operation),
                "{operation:?}"
            );
        }
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<&str> = PrivilegedOperation::ALL
            .iter()
            .map(|op| op.as_str())
            .collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), PrivilegedOperation::ALL.len());

        let mut names: Vec<&str> = AgentOperation::ALL.iter().map(|op| op.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), AgentOperation::ALL.len());
    }

    #[test]
    fn agent_operation_classes_are_as_documented() {
        use AgentOperation as Op;
        assert_eq!(Op::MemoryRead.class(), ActionClass::Observe);
        assert_eq!(Op::MemoryWrite.class(), ActionClass::Reversible);
        assert_eq!(Op::ScheduleWrite.class(), ActionClass::Consequential);
        for operation in [
            Op::KeyringSecretRead,
            Op::PolicyWrite,
            Op::PermissionWrite,
            Op::StandingApprovalWrite,
            Op::IndicatorDisable,
            Op::AuditLogDisable,
            Op::TrustedPathOperate,
            Op::ApfsWriteEnable,
        ] {
            assert_eq!(operation.class(), ActionClass::Prohibited, "{operation:?}");
        }
    }
}
