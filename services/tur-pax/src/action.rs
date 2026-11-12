// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

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
    /// A typed tool call on a TUR service (tier 1).
    Tool(ToolCall),
    /// An action through the accessibility tree (tier 2).
    Accessibility(UiAction),
    /// A screen capture or emulated input through the compositor (tier 3).
    ScreenInput(UiAction),
    /// A command run in a sandbox (the fourth way of acting). A command in the
    /// privileged sandbox is a free-text privileged request.
    Command(CommandAction),
    /// A typed operation of the privileged helper (addendum section 5.4).
    Operation(TypedOperation),
    /// One of the agent's own built-in operations, such as writing to memory.
    Agent(AgentOperation),
}

impl Action {
    /// The tier recorded in the audit log.
    ///
    /// 1 to 3 are the tiers of the addendum; 4 is a command or a typed
    /// operation of the privileged helper; 0 is an operation of the agent
    /// itself, which uses none of the ways of acting (addendum section 17,
    /// item 11).
    pub const fn tier(&self) -> u8 {
        match self {
            Action::Agent(_) => 0,
            Action::Tool(_) => 1,
            Action::Accessibility(_) => 2,
            Action::ScreenInput(_) => 3,
            Action::Command(_) | Action::Operation(_) => 4,
        }
    }

    /// The kind of privileged request this action is, if it is one.
    ///
    /// A privileged request is anything passed to the privileged helper: a
    /// command in the privileged sandbox, or a typed operation.
    pub const fn privileged_request(&self) -> Option<PrivilegedRequestKind> {
        match self {
            Action::Command(command) => match command.sandbox {
                Sandbox::Privileged(_) => Some(PrivilegedRequestKind::FreeText),
                Sandbox::ReadOnly | Sandbox::User => None,
            },
            Action::Operation(_) => Some(PrivilegedRequestKind::Typed),
            Action::Tool(_)
            | Action::Accessibility(_)
            | Action::ScreenInput(_)
            | Action::Agent(_) => None,
        }
    }
}

/// The two kinds of request the privileged helper accepts (addendum
/// section 5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrivilegedRequestKind {
    /// One operation of the closed set in [`TYPED_OPERATIONS`], which the
    /// helper can check against its type.
    Typed,
    /// A command line. Nothing can verify what it will do, so the user approves
    /// each one.
    FreeText,
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
    /// Root, through the privileged helper: a free-text command. The operation
    /// type is what the request declares; it selects the helper's confinement.
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

/// The type of operation a privileged request asks the privileged helper for.
///
/// For a free-text command the operation type is a field of the request: it
/// selects the confinement the helper applies and is not derived from the
/// command text. For a typed operation it follows from the operation and its
/// target ([`TypedOperation::operation_type`]).
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

/// The text shown and recorded in place of a value that is marked secret.
pub const SECRET_PLACEHOLDER: &str = "<secret>";

/// A typed operation requested from the privileged helper.
///
/// Unlike a command, a typed operation says what it does: its kind is one of a
/// closed set, and it acts on one named thing. [`TYPED_OPERATIONS`] states, for
/// each kind, what the name must look like and what the helper confines the
/// operation to. A request that does not have that shape is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedOperation {
    /// Which operation of the closed set.
    pub kind: TypedOperationKind,
    /// The one thing the operation acts on: a unit, a configuration file, a
    /// package or a network profile, as the kind requires.
    pub target: String,
    /// The new content, for a kind that writes one; `None` for every other.
    pub content: Option<OperationContent>,
}

impl TypedOperation {
    /// Whether the request has the shape its kind is confined to: a target
    /// that satisfies the kind's [`TargetRule`], and a content exactly when
    /// the kind takes one.
    pub fn is_well_formed(&self) -> bool {
        let spec = self.kind.spec();
        spec.target.accepts(&self.target) && self.content.is_some() == spec.takes_content
    }

    /// The operation type the request is treated as.
    ///
    /// This is the type of its kind, unless the target is one of the
    /// [`PROTECTED_TARGETS`]: then it is the system-critical type recorded
    /// there, which no repair session covers.
    pub fn operation_type(&self) -> PrivilegedOperation {
        let spec = self.kind.spec();
        PROTECTED_TARGETS
            .iter()
            .find(|entry| entry.rule == spec.target && entry.covers(&self.target))
            .map_or(spec.operation, |entry| entry.operation)
    }
}

/// The content a typed operation writes.
#[derive(Clone, PartialEq, Eq)]
pub struct OperationContent {
    /// The text to write.
    pub text: String,
    /// The text is a secret. It is replaced by [`SECRET_PLACEHOLDER`] in the
    /// prompt and in the audit record.
    ///
    /// Only the orchestrator may set this, and only for a value the user
    /// entered on a protected surface. A value proposed by the model is never
    /// marked: the model could otherwise keep what it writes out of the record.
    pub secret: bool,
}

impl std::fmt::Debug for OperationContent {
    // A secret must not reach a log through a debug print either.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = if self.secret {
            SECRET_PLACEHOLDER
        } else {
            self.text.as_str()
        };
        f.debug_struct("OperationContent")
            .field("text", &text)
            .field("secret", &self.secret)
            .finish()
    }
}

/// The closed set of typed operations the privileged helper accepts.
///
/// The discriminants index [`TYPED_OPERATIONS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypedOperationKind {
    /// Start a named unit.
    ServiceStart,
    /// Stop a named unit.
    ServiceStop,
    /// Restart a named unit.
    ServiceRestart,
    /// Enable a named unit, so that it starts with the system.
    ServiceEnable,
    /// Disable a named unit.
    ServiceDisable,
    /// Replace the content of a named configuration file.
    ConfigurationFileChange,
    /// Add a named package as a layer on the OS image.
    PackageLayerInstall,
    /// Remove a named package layer.
    PackageLayerRemove,
    /// Reset a named network profile to its defaults.
    NetworkProfileReset,
}

impl TypedOperationKind {
    /// Every kind, in the order of [`TYPED_OPERATIONS`].
    pub const ALL: [TypedOperationKind; 9] = [
        TypedOperationKind::ServiceStart,
        TypedOperationKind::ServiceStop,
        TypedOperationKind::ServiceRestart,
        TypedOperationKind::ServiceEnable,
        TypedOperationKind::ServiceDisable,
        TypedOperationKind::ConfigurationFileChange,
        TypedOperationKind::PackageLayerInstall,
        TypedOperationKind::PackageLayerRemove,
        TypedOperationKind::NetworkProfileReset,
    ];

    /// What the kind is confined to.
    pub const fn spec(self) -> &'static TypedOperationSpec {
        &TYPED_OPERATIONS[self as usize]
    }

    /// The kind as text, for prompts and the audit log.
    pub const fn as_str(self) -> &'static str {
        self.spec().name
    }
}

/// What one kind of typed operation is confined to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypedOperationSpec {
    /// The kind this entry describes.
    pub kind: TypedOperationKind,
    /// The kind as text.
    pub name: &'static str,
    /// The operation type, which selects the helper's confinement. Always one
    /// a repair session may cover.
    pub operation: PrivilegedOperation,
    /// What the target must be.
    pub target: TargetRule,
    /// Whether the operation writes a content.
    pub takes_content: bool,
    /// What the helper must confine the operation to, in words.
    pub confinement: &'static str,
}

const fn typed(
    kind: TypedOperationKind,
    name: &'static str,
    operation: PrivilegedOperation,
    target: TargetRule,
    takes_content: bool,
    confinement: &'static str,
) -> TypedOperationSpec {
    TypedOperationSpec {
        kind,
        name,
        operation,
        target,
        takes_content,
        confinement,
    }
}

/// The typed operations, one entry for each [`TypedOperationKind`], in the
/// order of the enum. This is the only place the set is defined.
pub const TYPED_OPERATIONS: &[TypedOperationSpec] = &[
    typed(
        TypedOperationKind::ServiceStart,
        "service_start",
        PrivilegedOperation::ServiceControl,
        TargetRule::Unit,
        false,
        "Starts the one named unit through the service manager. No other unit, no unit file, \
         no other argument.",
    ),
    typed(
        TypedOperationKind::ServiceStop,
        "service_stop",
        PrivilegedOperation::ServiceControl,
        TargetRule::Unit,
        false,
        "Stops the one named unit through the service manager. No other unit, no unit file, \
         no other argument.",
    ),
    typed(
        TypedOperationKind::ServiceRestart,
        "service_restart",
        PrivilegedOperation::ServiceControl,
        TargetRule::Unit,
        false,
        "Restarts the one named unit through the service manager. No other unit, no unit \
         file, no other argument.",
    ),
    typed(
        TypedOperationKind::ServiceEnable,
        "service_enable",
        PrivilegedOperation::ServiceControl,
        TargetRule::Unit,
        false,
        "Enables the one named unit, which must already be installed. Creates no unit file \
         and changes none.",
    ),
    typed(
        TypedOperationKind::ServiceDisable,
        "service_disable",
        PrivilegedOperation::ServiceControl,
        TargetRule::Unit,
        false,
        "Disables the one named unit. Removes no unit file and changes none.",
    ),
    typed(
        TypedOperationKind::ConfigurationFileChange,
        "configuration_file_change",
        PrivilegedOperation::SystemConfiguration,
        TargetRule::ConfigurationFile,
        true,
        "Replaces the content of the one named regular file under /etc, keeping the previous \
         content so that the change can be reversed. Follows no symbolic link, creates no \
         directory, sets no owner or mode, and runs nothing.",
    ),
    typed(
        TypedOperationKind::PackageLayerInstall,
        "package_layer_install",
        PrivilegedOperation::SoftwareManagement,
        TargetRule::Package,
        false,
        "Layers the one named package from the configured, signed repositories on the OS \
         image. Adds no repository, installs no local file, and takes no other option.",
    ),
    typed(
        TypedOperationKind::PackageLayerRemove,
        "package_layer_remove",
        PrivilegedOperation::SoftwareManagement,
        TargetRule::Package,
        false,
        "Removes the one named package layer. Removes nothing that is part of the base image.",
    ),
    typed(
        TypedOperationKind::NetworkProfileReset,
        "network_profile_reset",
        PrivilegedOperation::NetworkConfiguration,
        TargetRule::NetworkProfile,
        false,
        "Resets the one named connection profile to the defaults of its type. Changes no \
         other profile, no firewall rule and no global network setting.",
    ),
];

/// What the target of a typed operation must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetRule {
    /// A unit name: 1 to 255 of the ASCII letters, digits and `:`, `_`, `.`,
    /// `@`, `-`, not starting with a hyphen, ending in one of the
    /// [`UNIT_SUFFIXES`].
    Unit,
    /// An absolute path below [`CONFIGURATION_ROOT`] of at most 4096 bytes.
    /// Each component is one or more of the ASCII letters, digits and `.`,
    /// `_`, `-`, `+`, `@`, `:`, and is neither `.` nor `..`.
    ConfigurationFile,
    /// A package name: an ASCII letter or digit, then up to 254 of the ASCII
    /// letters, digits and `.`, `_`, `+`, `-`.
    Package,
    /// A connection profile name: 1 to 255 characters, each a printable ASCII
    /// character other than `/`, or a letter or digit of another script; with
    /// no space at either end and no leading hyphen.
    NetworkProfile,
}

/// The unit types a typed operation may name.
pub const UNIT_SUFFIXES: &[&str] = &[".service", ".socket", ".timer", ".path"];

/// The directory a configuration file named by a typed operation must be in.
pub const CONFIGURATION_ROOT: &str = "/etc/";

impl TargetRule {
    /// Every rule.
    pub const ALL: [TargetRule; 4] = [
        TargetRule::Unit,
        TargetRule::ConfigurationFile,
        TargetRule::Package,
        TargetRule::NetworkProfile,
    ];

    /// What the target is called in a prompt.
    pub const fn noun(self) -> &'static str {
        match self {
            TargetRule::Unit => "unit",
            TargetRule::ConfigurationFile => "file",
            TargetRule::Package => "package",
            TargetRule::NetworkProfile => "network profile",
        }
    }

    /// Whether `target` satisfies the rule.
    pub fn accepts(self, target: &str) -> bool {
        match self {
            TargetRule::Unit => is_unit_name(target),
            TargetRule::ConfigurationFile => is_configuration_path(target),
            TargetRule::Package => is_package_name(target),
            TargetRule::NetworkProfile => is_profile_name(target),
        }
    }
}

fn is_unit_name(name: &str) -> bool {
    name.len() <= 255
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'_' | b'.' | b'@' | b'-'))
        && UNIT_SUFFIXES.iter().any(|suffix| {
            name.strip_suffix(*suffix)
                .is_some_and(|stem| !stem.is_empty())
        })
}

fn is_configuration_path(path: &str) -> bool {
    path.len() <= 4096
        && path
            .strip_prefix(CONFIGURATION_ROOT)
            .is_some_and(|rest| rest.split('/').all(is_path_component))
}

fn is_path_component(component: &str) -> bool {
    !component.is_empty()
        && component != "."
        && component != ".."
        && component.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+' | b'@' | b':')
        })
}

fn is_package_name(name: &str) -> bool {
    name.len() <= 255
        && name
            .bytes()
            .next()
            .is_some_and(|first| first.is_ascii_alphanumeric())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'))
}

fn is_profile_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= 255
        && name.trim() == name
        && !name.starts_with('-')
        && name.chars().all(|c| match c {
            // A profile is named, never addressed by a path.
            '/' => false,
            ' '..='~' => true,
            // No control character, and none that does not show.
            _ => c.is_alphanumeric(),
        })
}

/// A target that makes a typed operation system-critical.
///
/// A typed operation on such a target is treated as the operation type
/// recorded here, which no repair session covers: the request is refused, and
/// the operation is reached, if at all, through the class 3 tool of the
/// service that owns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtectedTarget {
    /// The kind of target the entry applies to.
    pub rule: TargetRule,
    /// The beginning of the protected names. For a unit or a package the
    /// comparison ignores ASCII case; for a path it is exact.
    pub prefix: &'static str,
    /// The system-critical operation type the request is treated as.
    pub operation: PrivilegedOperation,
}

impl ProtectedTarget {
    /// Whether the entry covers `target`.
    ///
    /// The comparison is on the beginning of the name, so an entry covers more
    /// than it names: `polkit` also covers a unit called `polkit-extra.service`.
    /// That errs towards refusing.
    pub fn covers(&self, target: &str) -> bool {
        match self.rule {
            TargetRule::ConfigurationFile | TargetRule::NetworkProfile => {
                target.starts_with(self.prefix)
            }
            TargetRule::Unit | TargetRule::Package => target
                .as_bytes()
                .get(..self.prefix.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(self.prefix.as_bytes())),
        }
    }
}

const fn protect(
    rule: TargetRule,
    prefix: &'static str,
    operation: PrivilegedOperation,
) -> ProtectedTarget {
    ProtectedTarget {
        rule,
        prefix,
        operation,
    }
}

/// The targets a repair session never covers. This is the only place the list
/// is defined.
///
/// The list is a deny list and therefore incomplete by nature. It is the
/// engine's layer; the helper's confinement of each operation type is the one
/// that has to hold (`docs/architecture/agent-policy.md`, known limits).
///
/// Entries with [`PrivilegedOperation::Unclassified`] are files and units that
/// are run as root rather than read as configuration. Changing one is not a
/// configuration change, so it is not available as a typed operation.
pub const PROTECTED_TARGETS: &[ProtectedTarget] = &[
    // Units: authentication, authorisation, the firewall and the audit trail.
    protect(
        TargetRule::Unit,
        "polkit",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "firewalld",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "nftables",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "iptables",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "ip6tables",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "ebtables",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "auditd",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "audit-",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "systemd-journal",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "systemd-logind",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "systemd-homed",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "sshd",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "sssd",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "fprintd",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "selinux",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "apparmor",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "fapolicyd",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "usbguard",
        PrivilegedOperation::SecurityPolicy,
    ),
    // Added when the unit names were checked against the Fedora 44 and 43
    // package file lists on 2026-11-12 (agent-policy.md, section 2.5): the
    // ARP firewall of iptables-services, the user database, and the login
    // screens, which are where a person authenticates.
    protect(
        TargetRule::Unit,
        "arptables",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "systemd-userdbd",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "sddm",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "greetd",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "plasmalogin",
        PrivilegedOperation::SecurityPolicy,
    ),
    // The measurements a disk key is sealed to, and the broker of the device
    // that holds it.
    protect(
        TargetRule::Unit,
        "systemd-pcr",
        PrivilegedOperation::Encryption,
    ),
    protect(
        TargetRule::Unit,
        "tpm2-abrmd",
        PrivilegedOperation::Encryption,
    ),
    // The project's own services, the assistant and its policy owner included.
    protect(
        TargetRule::Unit,
        "tur-",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Unit,
        "tur-",
        PrivilegedOperation::SecurityPolicy,
    ),
    // Units: storage, encryption, boot, firmware and the OS image.
    protect(
        TargetRule::Unit,
        "systemd-cryptsetup",
        PrivilegedOperation::Encryption,
    ),
    protect(TargetRule::Unit, "clevis", PrivilegedOperation::Encryption),
    protect(
        TargetRule::Unit,
        "systemd-repart",
        PrivilegedOperation::Partitioning,
    ),
    protect(
        TargetRule::Unit,
        "systemd-growfs",
        PrivilegedOperation::Partitioning,
    ),
    protect(TargetRule::Unit, "lvm2", PrivilegedOperation::Partitioning),
    protect(TargetRule::Unit, "mdadm", PrivilegedOperation::Partitioning),
    protect(TargetRule::Unit, "mdmon", PrivilegedOperation::Partitioning),
    protect(
        TargetRule::Unit,
        "multipathd",
        PrivilegedOperation::Partitioning,
    ),
    protect(
        TargetRule::Unit,
        "udisks",
        PrivilegedOperation::Partitioning,
    ),
    protect(
        TargetRule::Unit,
        "systemd-makefs",
        PrivilegedOperation::Formatting,
    ),
    protect(
        TargetRule::Unit,
        "systemd-mkswap",
        PrivilegedOperation::Formatting,
    ),
    protect(
        TargetRule::Unit,
        "systemd-fsck",
        PrivilegedOperation::Formatting,
    ),
    protect(TargetRule::Unit, "bootupd", PrivilegedOperation::Bootloader),
    protect(
        TargetRule::Unit,
        "bootloader-update",
        PrivilegedOperation::Bootloader,
    ),
    protect(
        TargetRule::Unit,
        "systemd-boot",
        PrivilegedOperation::Bootloader,
    ),
    protect(TargetRule::Unit, "grub", PrivilegedOperation::Bootloader),
    protect(TargetRule::Unit, "fwupd", PrivilegedOperation::Firmware),
    protect(
        TargetRule::Unit,
        "rpm-ostree",
        PrivilegedOperation::OsUpdate,
    ),
    protect(TargetRule::Unit, "ostree", PrivilegedOperation::OsUpdate),
    protect(TargetRule::Unit, "bootc", PrivilegedOperation::OsUpdate),
    // A bus-activation alias can stand for any service.
    protect(TargetRule::Unit, "dbus", PrivilegedOperation::Unclassified),
    // Files: authentication, authorisation, accounts, trust and the audit trail.
    protect(
        TargetRule::ConfigurationFile,
        "/etc/polkit-1",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/sudo",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/pam.d",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/security",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/shadow",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/gshadow",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/passwd",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/group",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/subuid",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/subgid",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/login.defs",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/nsswitch.conf",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/sssd",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/krb5",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/selinux",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/apparmor",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/fapolicyd",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/usbguard",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/audit",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/ssh",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/firewalld",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/nftables",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/sysconfig/nftables",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/sysconfig/iptables",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/sysconfig/ip6tables",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/sysctl",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/dbus-1",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/pki",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/ssl",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/ca-certificates",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/crypto-policies",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/containers",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/yum",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/dnf",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/flatpak",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/doas",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/sysusers.d",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/sysconfig/selinux",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/rpm",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/sddm",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/gdm",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/systemd/logind.conf",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/systemd/journald.conf",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/tur",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/tur",
        PrivilegedOperation::SecurityPolicy,
    ),
    // Files: storage, encryption, boot, firmware and the OS image.
    protect(
        TargetRule::ConfigurationFile,
        "/etc/crypttab",
        PrivilegedOperation::Encryption,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/cryptsetup-keys.d",
        PrivilegedOperation::Encryption,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/clevis",
        PrivilegedOperation::Encryption,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/fstab",
        PrivilegedOperation::Partitioning,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/lvm",
        PrivilegedOperation::Partitioning,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/mdadm",
        PrivilegedOperation::Partitioning,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/multipath",
        PrivilegedOperation::Partitioning,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/default/grub",
        PrivilegedOperation::Bootloader,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/grub",
        PrivilegedOperation::Bootloader,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/kernel",
        PrivilegedOperation::Bootloader,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/dracut",
        PrivilegedOperation::Bootloader,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/loader",
        PrivilegedOperation::Bootloader,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/modprobe.d",
        PrivilegedOperation::Bootloader,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/modules-load.d",
        PrivilegedOperation::Bootloader,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/fwupd",
        PrivilegedOperation::Firmware,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/ostree",
        PrivilegedOperation::OsUpdate,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/rpm-ostree",
        PrivilegedOperation::OsUpdate,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/bootc",
        PrivilegedOperation::OsUpdate,
    ),
    // Files that are run as root rather than read as configuration.
    protect(
        TargetRule::ConfigurationFile,
        "/etc/systemd/system",
        PrivilegedOperation::Unclassified,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/systemd/user",
        PrivilegedOperation::Unclassified,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/udev",
        PrivilegedOperation::Unclassified,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/cron",
        PrivilegedOperation::Unclassified,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/anacrontab",
        PrivilegedOperation::Unclassified,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/tmpfiles.d",
        PrivilegedOperation::Unclassified,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/binfmt.d",
        PrivilegedOperation::Unclassified,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/profile",
        PrivilegedOperation::Unclassified,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/bash",
        PrivilegedOperation::Unclassified,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/zsh",
        PrivilegedOperation::Unclassified,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/environment",
        PrivilegedOperation::Unclassified,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/ld.so",
        PrivilegedOperation::Unclassified,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/init",
        PrivilegedOperation::Unclassified,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/alternatives",
        PrivilegedOperation::Unclassified,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/rc",
        PrivilegedOperation::Unclassified,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/xdg/autostart",
        PrivilegedOperation::Unclassified,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/X11/xinit",
        PrivilegedOperation::Unclassified,
    ),
    protect(
        TargetRule::ConfigurationFile,
        "/etc/NetworkManager/dispatcher.d",
        PrivilegedOperation::Unclassified,
    ),
    // Packages: the same areas.
    protect(
        TargetRule::Package,
        "polkit",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Package,
        "sudo",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Package,
        "pam",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Package,
        "shadow-utils",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Package,
        "selinux",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Package,
        "audit",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Package,
        "firewalld",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Package,
        "nftables",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Package,
        "iptables",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Package,
        "openssh",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Package,
        "ca-certificates",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Package,
        "crypto-policies",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Package,
        "usbguard",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Package,
        "fapolicyd",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Package,
        "sssd",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Package,
        "tur-",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Package,
        "tur-",
        PrivilegedOperation::SecurityPolicy,
    ),
    protect(
        TargetRule::Package,
        "cryptsetup",
        PrivilegedOperation::Encryption,
    ),
    protect(
        TargetRule::Package,
        "clevis",
        PrivilegedOperation::Encryption,
    ),
    protect(
        TargetRule::Package,
        "lvm2",
        PrivilegedOperation::Partitioning,
    ),
    protect(
        TargetRule::Package,
        "mdadm",
        PrivilegedOperation::Partitioning,
    ),
    protect(
        TargetRule::Package,
        "udisks",
        PrivilegedOperation::Partitioning,
    ),
    protect(TargetRule::Package, "grub", PrivilegedOperation::Bootloader),
    protect(TargetRule::Package, "shim", PrivilegedOperation::Bootloader),
    protect(
        TargetRule::Package,
        "bootupd",
        PrivilegedOperation::Bootloader,
    ),
    protect(
        TargetRule::Package,
        "efibootmgr",
        PrivilegedOperation::Bootloader,
    ),
    protect(
        TargetRule::Package,
        "dracut",
        PrivilegedOperation::Bootloader,
    ),
    protect(TargetRule::Package, "fwupd", PrivilegedOperation::Firmware),
    protect(
        TargetRule::Package,
        "linux-firmware",
        PrivilegedOperation::Firmware,
    ),
    protect(
        TargetRule::Package,
        "microcode",
        PrivilegedOperation::Firmware,
    ),
    protect(TargetRule::Package, "kernel", PrivilegedOperation::OsUpdate),
    protect(
        TargetRule::Package,
        "systemd",
        PrivilegedOperation::OsUpdate,
    ),
    protect(TargetRule::Package, "glibc", PrivilegedOperation::OsUpdate),
    protect(TargetRule::Package, "dbus", PrivilegedOperation::OsUpdate),
    protect(
        TargetRule::Package,
        "rpm-ostree",
        PrivilegedOperation::OsUpdate,
    ),
    protect(TargetRule::Package, "ostree", PrivilegedOperation::OsUpdate),
    protect(TargetRule::Package, "bootc", PrivilegedOperation::OsUpdate),
];

/// A built-in operation of the agent itself.
///
/// These are not service tools, so no manifest declares them. Their classes are
/// fixed here, in [`AgentOperation::class`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentOperation {
    /// Recall from the assistant's memory.
    MemoryRead,
    /// Record an observed entry: something the assistant's own completed
    /// action showed. The one kind of entry an unattended task may record
    /// (addendum section 18, item 4).
    MemoryRecordObserved,
    /// Record an entry on the user's word: something the user stated, a
    /// procedure the user accepted, or content that waits for the user's
    /// confirmation. With nobody present there is no such word, so the
    /// engine defers it in an unattended task.
    MemoryRecordStated,
    /// Change or remove an observed entry.
    MemoryChangeObserved,
    /// Change or remove an entry the user stated or confirmed, or a saved
    /// procedure. This can change what the assistant does later ("always ask
    /// before emailing my manager"), so it is class 2: the user is asked.
    MemoryChangeStated,
    /// Confirm a pending entry taken from content. The user does that in
    /// settings; the assistant cannot (addendum section 18, item 2).
    MemoryConfirmPending,
    /// Propose a reminder or a scheduled task. Nothing takes effect until the
    /// user confirms the proposal on screen (addendum section 18, item 3).
    ScheduleProposal,
    /// Create, change or remove a reminder or scheduled task. That is the
    /// user's act, in the component that owns schedules; the assistant never
    /// performs it (addendum section 18, items 1 and 3).
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
    pub const ALL: [AgentOperation; 16] = [
        AgentOperation::MemoryRead,
        AgentOperation::MemoryRecordObserved,
        AgentOperation::MemoryRecordStated,
        AgentOperation::MemoryChangeObserved,
        AgentOperation::MemoryChangeStated,
        AgentOperation::MemoryConfirmPending,
        AgentOperation::ScheduleProposal,
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
            AgentOperation::MemoryRecordObserved
            | AgentOperation::MemoryRecordStated
            | AgentOperation::MemoryChangeObserved
            | AgentOperation::ScheduleProposal => ActionClass::Reversible,
            AgentOperation::MemoryChangeStated => ActionClass::Consequential,
            AgentOperation::MemoryConfirmPending
            | AgentOperation::ScheduleWrite
            | AgentOperation::KeyringSecretRead
            | AgentOperation::PolicyWrite
            | AgentOperation::PermissionWrite
            | AgentOperation::StandingApprovalWrite
            | AgentOperation::IndicatorDisable
            | AgentOperation::AuditLogDisable
            | AgentOperation::TrustedPathOperate
            | AgentOperation::ApfsWriteEnable => ActionClass::Prohibited,
        }
    }

    /// Whether the operation records something on the user's word, so that
    /// it needs the user to be there. An unattended task is deferred instead
    /// (addendum section 18, item 4).
    pub const fn needs_the_user_present(self) -> bool {
        matches!(self, AgentOperation::MemoryRecordStated)
    }

    /// The operation as text, for prompts and the audit log.
    pub const fn as_str(self) -> &'static str {
        match self {
            AgentOperation::MemoryRead => "memory_read",
            AgentOperation::MemoryRecordObserved => "memory_record_observed",
            AgentOperation::MemoryRecordStated => "memory_record_stated",
            AgentOperation::MemoryChangeObserved => "memory_change_observed",
            AgentOperation::MemoryChangeStated => "memory_change_stated",
            AgentOperation::MemoryConfirmPending => "memory_confirm_pending",
            AgentOperation::ScheduleProposal => "schedule_propose",
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

    fn operation(kind: TypedOperationKind, target: &str) -> TypedOperation {
        TypedOperation {
            kind,
            target: target.to_owned(),
            content: kind.spec().takes_content.then(|| OperationContent {
                text: "key = value\n".to_owned(),
                secret: false,
            }),
        }
    }

    #[test]
    fn every_typed_operation_has_its_entry_in_the_table() {
        assert_eq!(TYPED_OPERATIONS.len(), TypedOperationKind::ALL.len());
        let mut names = Vec::new();
        for (index, kind) in TypedOperationKind::ALL.into_iter().enumerate() {
            assert_eq!(kind as usize, index);
            let spec = kind.spec();
            assert_eq!(spec.kind, kind);
            assert_eq!(kind.as_str(), spec.name);
            // A typed operation is never system-critical by its kind.
            assert!(spec.operation.repair_session_may_cover(), "{kind:?}");
            assert!(spec.confinement.ends_with('.'), "{kind:?}");
            assert!(
                spec.name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'_'),
                "{kind:?}"
            );
            names.push(spec.name);
        }
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), TypedOperationKind::ALL.len());
    }

    #[test]
    fn every_protected_target_is_system_critical() {
        for entry in PROTECTED_TARGETS {
            assert!(
                !entry.operation.repair_session_may_cover(),
                "{}",
                entry.prefix
            );
            assert!(!entry.prefix.is_empty());
            if entry.rule == TargetRule::ConfigurationFile {
                assert!(
                    entry.prefix.starts_with(CONFIGURATION_ROOT),
                    "{}",
                    entry.prefix
                );
            }
        }
        // Every rule that has protected names has at least one entry.
        for rule in [
            TargetRule::Unit,
            TargetRule::ConfigurationFile,
            TargetRule::Package,
        ] {
            assert!(PROTECTED_TARGETS.iter().any(|entry| entry.rule == rule));
        }
    }

    #[test]
    fn a_target_must_have_the_shape_of_its_rule() {
        let long_unit = format!("{}.service", "a".repeat(248));
        let long_package = "a".repeat(256);
        let long_profile = "a".repeat(256);
        let long_path = format!("/etc/{}", "a".repeat(4092));
        let cases: [(TargetRule, &[&str], &[&str]); 4] = [
            (
                TargetRule::Unit,
                &[
                    "cups.service",
                    "NetworkManager.service",
                    "getty@tty1.service",
                    "cups.socket",
                    "fstrim.timer",
                    "a.path",
                    "dbus-org.freedesktop.nm-dispatcher.service",
                ],
                &[
                    "",
                    ".service",
                    "cups",
                    "cups.mount",
                    "cups.target",
                    "cups.SERVICE",
                    "cups.service ",
                    " cups.service",
                    "cups.service; reboot",
                    "cups.service\n",
                    "-.service",
                    "--now.service",
                    "../cups.service",
                    "a/b.service",
                    "c\u{fc}ps.service",
                    "cups\u{200b}.service",
                    long_unit.as_str(),
                ],
            ),
            (
                TargetRule::ConfigurationFile,
                &[
                    "/etc/hosts",
                    "/etc/cups/cupsd.conf",
                    "/etc/NetworkManager/conf.d/10-dns.conf",
                    "/etc/a+b@c:d_e-f.conf",
                ],
                &[
                    "",
                    "/",
                    "/etc",
                    "/etc/",
                    "etc/hosts",
                    "/ETC/hosts",
                    "/etc//hosts",
                    "/etc/./hosts",
                    "/etc/../boot/loader/loader.conf",
                    "/etc/cups/../../boot/x",
                    "/etc/cups/",
                    "/usr/lib/x.conf",
                    "/etc/my file.conf",
                    "/etc/hosts\n",
                    "/etc/h\u{f6}sts",
                    "/etc/x;y",
                    "/etc/$(id)",
                    "/etc/x\u{0}y",
                    long_path.as_str(),
                ],
            ),
            (
                TargetRule::Package,
                &["cups", "gcc-c++", "python3.12", "libX11", "0ad"],
                &[
                    "",
                    "-cups",
                    ".hidden",
                    "./local.rpm",
                    "cups extra",
                    "cups;reboot",
                    "cups/../x",
                    "https://example.org/x.rpm",
                    "c\u{fc}ps",
                    long_package.as_str(),
                ],
            ),
            (
                TargetRule::NetworkProfile,
                &[
                    "eth0",
                    "Wired connection 1",
                    "Caf\u{e9} WLAN",
                    "\u{7121}\u{7dda} 5G",
                    "home (2.4 GHz)",
                ],
                &[
                    "",
                    " padded",
                    "padded ",
                    "\u{a0}padded",
                    "-option",
                    "line\nbreak",
                    "tab\tname",
                    "nul\u{0}",
                    "a/b",
                    "../system-connections/other",
                    "/etc/shadow",
                    "wifi\u{200b}",
                    "\u{202e}ifiw",
                    "wi\u{301}fi",
                    "wifi \u{1f4f6}",
                    long_profile.as_str(),
                ],
            ),
        ];
        for (rule, accepted, rejected) in cases {
            for target in accepted {
                assert!(rule.accepts(target), "{rule:?} must accept {target:?}");
            }
            for target in rejected {
                assert!(!rule.accepts(target), "{rule:?} must reject {target:?}");
            }
        }
    }

    #[test]
    fn the_real_names_of_the_protected_units_are_covered() {
        use PrivilegedOperation as Op;
        // Unit names as packages really ship them: seen in the Fedora 44 and
        // 43 package file lists on 2026-11-12 (apparmor in Debian's; the
        // cryptsetup template in systemd's sources). A wrong prefix in the
        // table would fail open; this is the check against the real names.
        let units = [
            ("polkit.service", Op::SecurityPolicy),
            ("polkit-agent-helper@.service", Op::SecurityPolicy),
            ("firewalld.service", Op::SecurityPolicy),
            ("nftables.service", Op::SecurityPolicy),
            ("iptables.service", Op::SecurityPolicy),
            ("ip6tables.service", Op::SecurityPolicy),
            ("ebtables.service", Op::SecurityPolicy),
            ("arptables.service", Op::SecurityPolicy),
            ("auditd.service", Op::SecurityPolicy),
            ("audit-rules.service", Op::SecurityPolicy),
            ("systemd-journald.service", Op::SecurityPolicy),
            ("systemd-journald.socket", Op::SecurityPolicy),
            ("systemd-logind.service", Op::SecurityPolicy),
            ("systemd-homed.service", Op::SecurityPolicy),
            ("systemd-userdbd.service", Op::SecurityPolicy),
            ("systemd-userdbd.socket", Op::SecurityPolicy),
            ("sshd.service", Op::SecurityPolicy),
            ("sshd.socket", Op::SecurityPolicy),
            ("sshd@.service", Op::SecurityPolicy),
            ("sssd.service", Op::SecurityPolicy),
            ("sssd-pam.socket", Op::SecurityPolicy),
            ("fprintd.service", Op::SecurityPolicy),
            ("selinux-autorelabel.service", Op::SecurityPolicy),
            ("selinux-autorelabel-mark.service", Op::SecurityPolicy),
            ("apparmor.service", Op::SecurityPolicy),
            ("usbguard.service", Op::SecurityPolicy),
            ("usbguard-dbus.service", Op::SecurityPolicy),
            ("sddm.service", Op::SecurityPolicy),
            ("greetd.service", Op::SecurityPolicy),
            ("plasmalogin.service", Op::SecurityPolicy),
            ("systemd-cryptsetup@luks\\x2droot.service", Op::Encryption),
            ("clevis-luks-askpass.service", Op::Encryption),
            ("clevis-luks-askpass.path", Op::Encryption),
            ("systemd-pcrphase.service", Op::Encryption),
            ("systemd-pcrphase-initrd.service", Op::Encryption),
            ("systemd-pcrextend@.service", Op::Encryption),
            ("systemd-pcrextend.socket", Op::Encryption),
            ("systemd-pcrlock.service", Op::Encryption),
            ("systemd-pcrmachine.service", Op::Encryption),
            ("tpm2-abrmd.service", Op::Encryption),
            ("fwupd.service", Op::Firmware),
            ("fwupd-refresh.service", Op::Firmware),
            ("fwupd-refresh.timer", Op::Firmware),
            ("systemd-boot-update.service", Op::Bootloader),
            ("rpm-ostreed.service", Op::OsUpdate),
            ("rpm-ostreed-automatic.timer", Op::OsUpdate),
            ("bootc-fetch-apply-updates.service", Op::OsUpdate),
            ("bootc-fetch-apply-updates.timer", Op::OsUpdate),
            ("bootc-finalize-staged.service", Op::OsUpdate),
            ("dbus-broker.service", Op::Unclassified),
            ("dbus-org.freedesktop.login1.service", Op::Unclassified),
        ];
        for (unit, expected) in units {
            let covering: Vec<&ProtectedTarget> = PROTECTED_TARGETS
                .iter()
                .filter(|entry| entry.rule == TargetRule::Unit && entry.covers(unit))
                .collect();
            assert!(!covering.is_empty(), "{unit} is not protected");
            assert_eq!(covering[0].operation, expected, "{unit}");
            // Written in another letter case it is the same unit.
            assert!(
                PROTECTED_TARGETS.iter().any(|entry| entry.rule == TargetRule::Unit
                    && entry.covers(&unit.to_ascii_uppercase())),
                "{unit}"
            );
            // Whatever it is treated as, no repair session covers it.
            assert!(!expected.repair_session_may_cover(), "{unit}");
        }
        // An ordinary unit is not caught by any of the prefixes.
        for unit in ["cups.service", "bluetooth.service", "NetworkManager.service"] {
            assert!(
                !PROTECTED_TARGETS
                    .iter()
                    .any(|entry| entry.rule == TargetRule::Unit && entry.covers(unit)),
                "{unit}"
            );
        }
    }

    #[test]
    fn a_protected_target_makes_a_typed_operation_system_critical() {
        use PrivilegedOperation as Op;
        use TypedOperationKind as Kind;
        let cases = [
            (Kind::ServiceRestart, "cups.service", Op::ServiceControl),
            (Kind::ServiceRestart, "polkit.service", Op::SecurityPolicy),
            (Kind::ServiceStop, "Polkit.service", Op::SecurityPolicy),
            (Kind::ServiceStop, "firewalld.service", Op::SecurityPolicy),
            (
                Kind::ServiceDisable,
                "systemd-journald.service",
                Op::SecurityPolicy,
            ),
            (Kind::ServiceStop, "tur-pax.service", Op::SecurityPolicy),
            (Kind::ServiceStart, "fwupd.service", Op::Firmware),
            (
                Kind::ServiceStart,
                "systemd-cryptsetup@luks.service",
                Op::Encryption,
            ),
            (Kind::ServiceEnable, "rpm-ostreed.service", Op::OsUpdate),
            (
                Kind::ServiceRestart,
                "dbus-org.fedoraproject.FirewallD1.service",
                Op::Unclassified,
            ),
            (
                Kind::ConfigurationFileChange,
                "/etc/cups/cupsd.conf",
                Op::SystemConfiguration,
            ),
            (
                Kind::ConfigurationFileChange,
                "/etc/sudoers",
                Op::SecurityPolicy,
            ),
            (
                Kind::ConfigurationFileChange,
                "/etc/sudoers.d/allow",
                Op::SecurityPolicy,
            ),
            (
                Kind::ConfigurationFileChange,
                "/etc/pam.d/system-auth",
                Op::SecurityPolicy,
            ),
            (
                Kind::ConfigurationFileChange,
                "/etc/polkit-1/rules.d/49-allow.rules",
                Op::SecurityPolicy,
            ),
            (
                Kind::ConfigurationFileChange,
                "/etc/crypttab",
                Op::Encryption,
            ),
            (
                Kind::ConfigurationFileChange,
                "/etc/fstab",
                Op::Partitioning,
            ),
            (
                Kind::ConfigurationFileChange,
                "/etc/default/grub",
                Op::Bootloader,
            ),
            (
                Kind::ConfigurationFileChange,
                "/etc/systemd/system/x.service",
                Op::Unclassified,
            ),
            (
                Kind::ConfigurationFileChange,
                "/etc/crontab",
                Op::Unclassified,
            ),
            (
                Kind::ConfigurationFileChange,
                "/etc/udev/rules.d/99-x.rules",
                Op::Unclassified,
            ),
            (Kind::PackageLayerInstall, "cups", Op::SoftwareManagement),
            (Kind::PackageLayerRemove, "kernel-core", Op::OsUpdate),
            (Kind::PackageLayerRemove, "Kernel", Op::OsUpdate),
            (Kind::PackageLayerInstall, "grub2-efi-x64", Op::Bootloader),
            (Kind::PackageLayerRemove, "polkit", Op::SecurityPolicy),
            (
                Kind::NetworkProfileReset,
                "Wired connection 1",
                Op::NetworkConfiguration,
            ),
        ];
        for (kind, target, expected) in cases {
            let request = operation(kind, target);
            assert!(request.is_well_formed(), "{kind:?} {target}");
            assert_eq!(request.operation_type(), expected, "{kind:?} {target}");
        }
    }

    #[test]
    fn a_content_is_required_exactly_where_the_kind_takes_one() {
        for kind in TypedOperationKind::ALL {
            let target = match kind.spec().target {
                TargetRule::Unit => "cups.service",
                TargetRule::ConfigurationFile => "/etc/cups/cupsd.conf",
                TargetRule::Package => "cups",
                TargetRule::NetworkProfile => "eth0",
            };
            let with = TypedOperation {
                kind,
                target: target.to_owned(),
                content: Some(OperationContent {
                    text: String::new(),
                    secret: false,
                }),
            };
            let without = TypedOperation {
                kind,
                target: target.to_owned(),
                content: None,
            };
            assert_eq!(with.is_well_formed(), kind.spec().takes_content, "{kind:?}");
            assert_eq!(
                without.is_well_formed(),
                !kind.spec().takes_content,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_secret_content_is_not_shown_by_a_debug_print() {
        let secret = OperationContent {
            text: "hunter2-correct-horse".to_owned(),
            secret: true,
        };
        let shown = format!("{secret:?}");
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(shown.contains(SECRET_PLACEHOLDER), "{shown}");

        let plain = OperationContent {
            text: "key = value".to_owned(),
            secret: false,
        };
        assert!(format!("{plain:?}").contains("key = value"));
    }

    #[test]
    fn a_privileged_request_is_a_privileged_command_or_a_typed_operation() {
        let command = |sandbox| {
            Action::Command(CommandAction {
                sandbox,
                argv: vec!["true".to_owned()],
                working_context: WorkingContext::Session,
            })
        };
        assert_eq!(command(Sandbox::ReadOnly).privileged_request(), None);
        assert_eq!(command(Sandbox::User).privileged_request(), None);
        for operation_type in PrivilegedOperation::ALL {
            assert_eq!(
                command(Sandbox::Privileged(operation_type)).privileged_request(),
                Some(PrivilegedRequestKind::FreeText)
            );
        }
        let typed = Action::Operation(operation(
            TypedOperationKind::ServiceRestart,
            "cups.service",
        ));
        assert_eq!(
            typed.privileged_request(),
            Some(PrivilegedRequestKind::Typed)
        );
        assert_eq!(typed.tier(), 4);
        for agent in AgentOperation::ALL {
            assert_eq!(Action::Agent(agent).privileged_request(), None);
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
        // Addendum section 18, items 3 and 4: recording is class 1, and so is
        // proposing a schedule, because a proposal changes nothing.
        for operation in [
            Op::MemoryRecordObserved,
            Op::MemoryRecordStated,
            Op::MemoryChangeObserved,
            Op::ScheduleProposal,
        ] {
            assert_eq!(operation.class(), ActionClass::Reversible, "{operation:?}");
        }
        // Changing or removing what the user stated is asked, not class 1.
        assert_eq!(Op::MemoryChangeStated.class(), ActionClass::Consequential);
        // Only a record made on the user's word needs the user there.
        for operation in Op::ALL {
            assert_eq!(
                operation.needs_the_user_present(),
                operation == Op::MemoryRecordStated,
                "{operation:?}"
            );
        }
        for operation in [
            // The user's acts, which the assistant never performs.
            Op::MemoryConfirmPending,
            Op::ScheduleWrite,
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
