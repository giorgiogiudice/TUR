// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The user-adjustable policy (addendum section 5.3) and its file format.
//!
//! The user can make the assistant stricter at any time, and can relax the
//! class 2 handling of accessibility and screen actions for a named
//! application. That is the only relaxation there is. Nothing in this file
//! format can relax class 3 or class 4. The file is owned by a separate system
//! component and written through system settings on the trusted path; the
//! assistant cannot write it (addendum section 17, item 5), and changing it is
//! a class 4 operation for the assistant.
//!
//! Two lists are built in and apply whatever the file says, an empty or
//! missing file included: [`DEFAULT_SENSITIVE_APPLICATIONS`] and
//! [`DEFAULT_EXCLUDED_APPLICATIONS`]. The user's lists add to them. Nothing in
//! the file format takes an entry away.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::codes::{Code, Problem};

/// The policy file schema version this build reads.
pub const POLICY_SCHEMA_VERSION: u64 = 1;

/// What a built-in application identifier was checked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Evidence {
    /// The name of a desktop file, in the upstream sources or in a
    /// distribution package: the application identifier a Wayland compositor
    /// reports.
    DesktopFile,
    /// An identifier the program sets for its windows itself: a
    /// `StartupWMClass` line, or an application identifier set in its code.
    WindowClass,
    /// The name of the program file. It is what a program that sets nothing
    /// reports as its window class under XWayland.
    Program,
    /// No source was found that shows the identifier. It is kept, because an
    /// entry too many only makes the assistant ask or look away once more;
    /// it protects nothing until it is confirmed.
    Unverified,
}

/// One built-in application identifier, and where it was checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownApplication {
    /// The identifier, as the compositor reports it.
    pub id: &'static str,
    /// What kind of name it is.
    pub evidence: Evidence,
    /// Where it was seen. The abbreviations are explained at
    /// [`SENSITIVE_APPLICATION_SOURCES`].
    pub source: &'static str,
}

/// Defines a list of built-in identifiers and its table of sources from one
/// set of rows, so that the two cannot differ.
macro_rules! known_applications {
    (
        $(#[$list_doc:meta])* list $list:ident;
        $(#[$table_doc:meta])* table $table:ident;
        $( ($id:literal, $evidence:ident, $source:literal) ),+ $(,)?
    ) => {
        $(#[$list_doc])*
        pub const $list: &[&str] = &[ $( $id ),+ ];

        $(#[$table_doc])*
        pub const $table: &[KnownApplication] = &[
            $( KnownApplication { id: $id, evidence: Evidence::$evidence, source: $source } ),+
        ];
    };
}

known_applications! {
    /// Applications that execute what is typed into them: terminal emulators, run
    /// dialogs and launchers (addendum section 17, item 1).
    ///
    /// Acting in one of them through the accessibility tree or the screen would be
    /// a way to run any command with no sandbox and no approval. They are
    /// therefore sensitive whatever the user policy says: every accessibility or
    /// screen action in them is class 2. The assistant uses the command tier
    /// instead, where the sandbox bounds what a command can do.
    ///
    /// The entries are the application identifiers the compositor reports: the
    /// desktop-file name on Wayland, the window class under XWayland. They are
    /// compared like the user's own entries, ignoring ASCII case, and an entry
    /// also matches its own form with `.desktop` appended. The list cannot
    /// be complete. An application that embeds a terminal, such as an editor with
    /// a terminal panel, is not on it; the pattern table catches the usual names
    /// of such panels, and the user can add the application.
    ///
    /// A wrong identifier fails open: the application would be treated as an
    /// ordinary one. Every entry was therefore checked against a published
    /// source; [`SENSITIVE_APPLICATION_SOURCES`] says which.
    list DEFAULT_SENSITIVE_APPLICATIONS;
    /// Where each entry of [`DEFAULT_SENSITIVE_APPLICATIONS`] was checked, on
    /// 2026-11-12. The same rows, in the same order.
    ///
    /// Abbreviations: `KDE:` a repository on invent.kde.org; `GNOME:` one on
    /// gitlab.gnome.org; `GH:` one on github.com; `CB:` one on codeberg.org;
    /// `XFCE:` one on gitlab.xfce.org; `FP44:` the file list of a Fedora 44
    /// package on packages.fedoraproject.org (`FP43:` Fedora 43); `DEB:` the
    /// file list of a Debian package on packages.debian.org (sid unless
    /// stated); `FH` the application is published on Flathub under this
    /// identifier (flathub.org/api/v2/appstream); `AUR:` a PKGBUILD on
    /// aur.archlinux.org, a secondary source.
    table SENSITIVE_APPLICATION_SOURCES;
    // KDE Plasma.
    ("org.kde.konsole", DesktopFile, "KDE:utilities/konsole desktop/org.kde.konsole.desktop; FP44:konsole; FH"),
    ("konsole", Program, "FP44:konsole /usr/bin/konsole"),
    ("org.kde.yakuake", DesktopFile, "KDE:utilities/yakuake data/org.kde.yakuake.desktop; FP44:yakuake; FH"),
    ("yakuake", Program, "FP44:yakuake /usr/bin/yakuake"),
    ("org.kde.krunner", DesktopFile, "KDE:plasma/plasma-workspace krunner/org.kde.krunner.desktop.cmake, installed under kglobalaccel"),
    ("krunner", Program, "FP44:plasma-workspace /usr/bin/krunner"),
    // GNOME.
    ("org.gnome.Terminal", DesktopFile, "GNOME:GNOME/gnome-terminal data/; FP44:gnome-terminal"),
    ("org.gnome.Terminal.Preferences", DesktopFile, "GNOME:GNOME/gnome-terminal data/; FP44:gnome-terminal"),
    ("gnome-terminal", Program, "FP44:gnome-terminal /usr/bin/gnome-terminal"),
    ("gnome-terminal-server", Program, "FP44:gnome-terminal /usr/libexec/gnome-terminal-server"),
    ("org.gnome.Console", DesktopFile, "GNOME:GNOME/console meson.build; FP44:gnome-console"),
    ("org.gnome.Console.Devel", DesktopFile, "GNOME:GNOME/console meson.build, development profile"),
    ("kgx", Program, "FP44:gnome-console /usr/bin/kgx"),
    ("kgx-devel", Program, "GNOME:GNOME/console meson.build, development profile"),
    ("org.gnome.Ptyxis", DesktopFile, "GNOME:chergert/ptyxis meson.build; FP44:ptyxis"),
    ("org.gnome.Ptyxis.Devel", DesktopFile, "GNOME:chergert/ptyxis meson.build, development builds"),
    ("app.devsuite.Ptyxis", DesktopFile, "FH: the Flathub build of Ptyxis"),
    ("ptyxis", Program, "FP44:ptyxis /usr/bin/ptyxis"),
    // Other terminal emulators.
    ("xterm", DesktopFile, "FP44:xterm xterm.desktop"),
    ("uxterm", Program, "FP44:xterm /usr/bin/uxterm"),
    ("debian-xterm", DesktopFile, "DEB:xterm"),
    ("debian-uxterm", DesktopFile, "DEB:xterm"),
    ("kitty", DesktopFile, "GH:kovidgoyal/kitty setup.py; FP44:kitty"),
    ("Alacritty", DesktopFile, "GH:alacritty/alacritty extra/linux/Alacritty.desktop, also StartupWMClass; the program is alacritty, the same entry without regard to case"),
    ("foot", DesktopFile, "CB:dnkl/foot foot.desktop, and the default app-id; FP44:foot"),
    ("footclient", DesktopFile, "CB:dnkl/foot doc/foot.ini.5.scd, the default app-id in server mode; FP44:foot"),
    ("foot-server", DesktopFile, "CB:dnkl/foot; FP44:foot"),
    ("org.codeberg.dnkl.foot", Unverified, "CB:dnkl/foot: the AppStream identifier only; no desktop file or app-id has it"),
    ("org.wezfurlong.wezterm", DesktopFile, "GH:wezterm/wezterm assets/wezterm.desktop installed under this name, also StartupWMClass; FH"),
    ("wezterm", Program, "GH:wezterm/wezterm assets/wezterm.desktop Exec"),
    ("com.mitchellh.ghostty", DesktopFile, "GH:ghostty-org/ghostty src/build/GhosttyResources.zig, dist/linux/app.desktop.in"),
    ("com.mitchellh.ghostty-debug", DesktopFile, "GH:ghostty-org/ghostty src/build/GhosttyResources.zig: debug builds"),
    ("ghostty", Program, "GH:ghostty-org/ghostty dist/linux/app.desktop.in Exec"),
    ("com.gexperts.Tilix", DesktopFile, "GH:gnunn1/tilix; FP44:tilix"),
    ("tilix", Program, "FP44:tilix /usr/bin/tilix"),
    ("terminator", DesktopFile, "GH:gnome-terminator/terminator; FP44:terminator"),
    ("xfce4-terminal", DesktopFile, "XFCE:apps/xfce4-terminal; FP44:xfce4-terminal"),
    ("lxterminal", DesktopFile, "FP44:lxterminal"),
    ("qterminal", DesktopFile, "FP44:qterminal"),
    ("qterminal-drop", DesktopFile, "FP44:qterminal"),
    ("mate-terminal", DesktopFile, "FP44:mate-terminal"),
    ("deepin-terminal", DesktopFile, "FP44:deepin-terminal"),
    ("io.elementary.terminal", DesktopFile, "GH:elementary/terminal data/io.elementary.terminal.desktop.in"),
    ("com.raggesilver.BlackBox", DesktopFile, "GNOME:raggesilver/blackbox data/com.raggesilver.BlackBox.desktop.in; FH"),
    ("blackbox-terminal", Program, "GNOME:raggesilver/blackbox data/com.raggesilver.BlackBox.desktop.in Exec"),
    ("com.system76.CosmicTerm", DesktopFile, "GH:pop-os/cosmic-term res/com.system76.CosmicTerm.desktop; FP44:cosmic-term"),
    ("cosmic-term", Program, "FP44:cosmic-term /usr/bin/cosmic-term"),
    ("cool-retro-term", DesktopFile, "FP44:cool-retro-term"),
    ("rxvt-unicode", DesktopFile, "FP44:rxvt-unicode; DEB:rxvt-unicode"),
    ("urxvt", Program, "FP44:rxvt-unicode /usr/bin/urxvt"),
    ("urxvtc", Program, "FP44:rxvt-unicode /usr/bin/urxvtc"),
    ("urxvtd", Program, "FP44:rxvt-unicode /usr/bin/urxvtd"),
    ("rxvt", Program, "FP44:rxvt-unicode"),
    ("st", DesktopFile, "FP44:st st.desktop"),
    ("st-256color", WindowClass, "git.suckless.org/st x.c, config.def.h: the window class is the terminal name"),
    ("guake", DesktopFile, "FP44:guake"),
    ("tilda", DesktopFile, "FP44:tilda"),
    ("terminology", DesktopFile, "FP44:terminology"),
    ("org.contourterminal.Contour", DesktopFile, "GH:contour-terminal/contour src/contour/CMakeLists.txt; FP44:contour-terminal; FH"),
    ("contour", Program, "FP44:contour-terminal /usr/bin/contour; it is not the name of the desktop file"),
    ("rio", DesktopFile, "GH:raphamorim/rio misc/rio.desktop; StartupWMClass is Rio, the same entry without regard to case"),
    ("com.rioterm.Rio", DesktopFile, "FH: the Flathub build of Rio"),
    ("sakura", DesktopFile, "FP44:sakura"),
    ("termite", DesktopFile, "GH:thestinger/termite termite.desktop"),
    ("roxterm", DesktopFile, "FP43:roxterm"),
    ("mlterm", DesktopFile, "DEB:mlterm, DEB:mlterm-common"),
    ("zutty", DesktopFile, "DEB:zutty"),
    ("dev.warp.Warp", DesktopFile, "GH:warpdotdev/warp app/channels/stable, also StartupWMClass"),
    ("dev.warp.WarpPreview", DesktopFile, "GH:warpdotdev/warp app/channels/preview"),
    ("dev.warp.WarpOss", DesktopFile, "GH:warpdotdev/warp app/channels/oss"),
    ("dev.warp.WarpDev", DesktopFile, "GH:warpdotdev/warp app/channels"),
    ("dev.warp.WarpLocal", DesktopFile, "GH:warpdotdev/warp app/channels"),
    ("warp-terminal", Program, "GH:warpdotdev/warp app/channels/stable Exec"),
    ("tabby", WindowClass, "GH:Eugeny/tabby electron-builder.yml StartupWMClass; the desktop file name was not seen"),
    ("hyper", Unverified, "GH:vercel/hyper electron-builder.json shows only the appId co.zeit.hyper"),
    // Run dialogs and launchers.
    ("rofi", DesktopFile, "FP44:rofi"),
    ("wofi", Program, "FP44:wofi /usr/bin/wofi; the package has no desktop file"),
    ("dmenu", Program, "FP44:dmenu"),
    ("dmenu_run", Program, "FP44:dmenu"),
    ("bemenu", Program, "FP44:bemenu"),
    ("bemenu-run", Program, "FP44:bemenu"),
    ("fuzzel", Program, "FP44:fuzzel; the package has no desktop file"),
    ("tofi", Program, "DEB:tofi"),
    ("tofi-run", Program, "DEB:tofi"),
    ("tofi-drun", Program, "DEB:tofi"),
    ("io.ulauncher.Ulauncher", DesktopFile, "GH:Ulauncher/Ulauncher: the current sources"),
    ("ulauncher", DesktopFile, "GH:Ulauncher/Ulauncher v5 setup.py; FP44:ulauncher"),
    ("albert", DesktopFile, "GH:albertlauncher/albert dist/xdg/albert.desktop"),
    ("kupfer", DesktopFile, "GH:kupferlauncher/kupfer auxdata/kupfer.desktop.in"),
    ("synapse", DesktopFile, "FP44:synapse"),
    ("gmrun", DesktopFile, "DEB:gmrun"),
    ("xfce4-appfinder", DesktopFile, "FP44:xfce4-appfinder"),
    ("xfce4-run", DesktopFile, "FP44:xfce4-appfinder"),
    ("xfrun4", Program, "FP44:xfce4-appfinder"),
    ("lxqt-runner", DesktopFile, "FP44:lxqt-runner"),
    // Consoles of the Windows compatibility runtime. These are the names of
    // Windows programs as Wine reports them; no desktop file names them and
    // none was looked for.
    ("cmd.exe", Unverified, "a Windows program name; not checked against a Wine session"),
    ("powershell.exe", Unverified, "a Windows program name; not checked against a Wine session"),
    ("pwsh.exe", Unverified, "a Windows program name; not checked against a Wine session"),
    ("conhost.exe", Unverified, "a Windows program name; not checked against a Wine session"),
    ("wineconsole", Unverified, "a Wine program name; not checked against a Wine session"),
    ("wineconsole.exe", Unverified, "a Wine program name; not checked against a Wine session"),
}

known_applications! {
    /// Applications whose windows hold secrets: password managers, the keyring,
    /// authenticators, and the prompts that ask for a password (addendum
    /// section 8).
    ///
    /// They are excluded whatever the user policy says: the assistant neither
    /// observes them nor acts in them. The prompts in the list are protected
    /// surfaces, which the compositor already keeps from the assistant; listing
    /// them here is a second layer. Private browser windows cannot be told from
    /// ordinary ones by an application identifier and are the compositor's to
    /// exclude.
    ///
    /// A wrong identifier fails open: the application would be observed. Every
    /// entry was therefore checked against a published source;
    /// [`EXCLUDED_APPLICATION_SOURCES`] says which.
    list DEFAULT_EXCLUDED_APPLICATIONS;
    /// Where each entry of [`DEFAULT_EXCLUDED_APPLICATIONS`] was checked, on
    /// 2026-11-12. The same rows, in the same order. The abbreviations are
    /// those of [`SENSITIVE_APPLICATION_SOURCES`].
    table EXCLUDED_APPLICATION_SOURCES;
    // Password managers and keyrings.
    ("org.keepassxc.KeePassXC", DesktopFile, "GH:keepassxreboot/keepassxc share/linux/org.keepassxc.KeePassXC.desktop.in; FP44:keepassxc; FH"),
    ("keepassxc", WindowClass, "GH:keepassxreboot/keepassxc share/linux/org.keepassxc.KeePassXC.desktop.in StartupWMClass"),
    ("keepass", DesktopFile, "FP44:keepass keepass.desktop, /usr/bin/keepass"),
    ("keepass2", DesktopFile, "DEB:keepass2"),
    ("org.keepass.KeePass", Unverified, "found in no source; not on Flathub"),
    ("org.kde.kwalletmanager", DesktopFile, "KDE:utilities/kwalletmanager from v25.04.0; FP44:kwalletmanager5; DEB:kwalletmanager"),
    ("org.kde.kwalletmanager5", DesktopFile, "KDE:utilities/kwalletmanager v23.08.0 and v24.02.0; still the Flathub identifier"),
    ("kwalletmanager5-kwalletd", DesktopFile, "KDE:utilities/kwalletmanager: the second desktop file, kwalletmanager5 --kwalletd"),
    ("kwalletmanager", WindowClass, "KDE:utilities/kwalletmanager: the KAboutData name"),
    ("kwalletmanager5", Program, "FP44:kwalletmanager5 /usr/bin/kwalletmanager5"),
    ("org.kde.kwalletd", DesktopFile, "KDE:frameworks/kwallet from v6.30.0, and setDesktopFileName in src/runtime/kwalletd/main.cpp"),
    ("org.kde.kwalletd5", DesktopFile, "KDE:frameworks/kwallet branch kf5"),
    ("org.kde.kwalletd6", DesktopFile, "KDE:frameworks/kwallet v6.0.0 to v6.10.0"),
    ("kwalletd", WindowClass, "KDE:frameworks/kwallet src/runtime/kwalletd/main.cpp: the KAboutData name"),
    ("kwalletd5", Program, "KDE:frameworks/kwallet branch kf5"),
    ("kwalletd6", Program, "FP44:kf6-kwallet /usr/bin/kwalletd6"),
    ("org.kde.ksecretd", DesktopFile, "KDE:frameworks/kwallet src/runtime/ksecretd/org.kde.ksecretd.desktop; FP44:kf6-kwallet"),
    ("ksecretd", Program, "FP44:kf6-kwallet /usr/bin/ksecretd"),
    ("org.gnome.seahorse.Application", DesktopFile, "GNOME:GNOME/seahorse 47.0.1; FP44:seahorse; DEB:seahorse; FH"),
    ("org.gnome.Seahorse", DesktopFile, "GNOME:GNOME/seahorse main meson.build: the new name, in no release seen"),
    ("org.gnome.Seahorse.Devel", DesktopFile, "GNOME:GNOME/seahorse main meson.build: development builds"),
    ("seahorse", Program, "FP44:seahorse /usr/bin/seahorse"),
    ("org.gnome.World.Secrets", DesktopFile, "GNOME:World/secrets; DEB:secrets; FH"),
    ("org.gnome.PasswordSafe", DesktopFile, "GNOME:World/secrets tags 3.99.2 and 5.1: the former name"),
    ("secrets", Program, "DEB:secrets /usr/bin/secrets"),
    ("com.bitwarden.desktop", DesktopFile, "GH:bitwarden/clients apps/desktop/electron-builder.json, resources/com.bitwarden.desktop.desktop, also StartupWMClass; FH"),
    ("bitwarden", DesktopFile, "AUR:bitwarden-bin installs bitwarden.desktop; the program is bitwarden"),
    ("com.onepassword.OnePassword", DesktopFile, "FH; AUR:1password: the 8.12 archive ships resources/com.onepassword.OnePassword.desktop"),
    ("com.1password.1Password", Unverified, "found in no source; not on Flathub. The identifier this list had before 2026-11-12"),
    ("1password", Unverified, "AUR:1password shows only an icon of this name"),
    ("me.proton.Pass", DesktopFile, "FH"),
    ("proton-pass", Unverified, "the name of the deb and rpm packages; the desktop file name was not seen"),
    ("Proton Pass", Program, "GH:ProtonMail/WebClients applications/pass-desktop/forge.config.ts: the name of the program file"),
    ("com.enpass.Enpass", Unverified, "found in no source; not on Flathub"),
    ("enpass", DesktopFile, "GH:NixOS/nixpkgs pkgs/by-name/en/enpass/package.nix names share/applications/enpass.desktop; a secondary source"),
    ("org.ijhack.QtPass", WindowClass, "GH:IJHack/QtPass qtpass.desktop StartupWMClass, written org.ijhack.qtpass: the same entry without regard to case"),
    ("org.qtpass.QtPass", Unverified, "GH:IJHack/QtPass: the AppStream identifier only; main/main.cpp takes the desktop file name from FLATPAK_ID when it is set"),
    ("qtpass", DesktopFile, "GH:IJHack/QtPass qtpass.desktop, main/main.cpp setDesktopFileName; FP44:qtpass"),
    // One-time-password authenticators.
    ("org.kde.keysmith", DesktopFile, "KDE:utilities/keysmith; FP44:keysmith; FH"),
    ("keysmith", Program, "FP44:keysmith /usr/bin/keysmith"),
    ("com.belmoussaoui.Authenticator", DesktopFile, "GNOME:World/Authenticator data/; FH"),
    ("authenticator", Program, "GNOME:World/Authenticator and GH:Yubico/yubioath-flutter: both programs have this name"),
    ("com.yubico.authenticator", DesktopFile, "GH:Yubico/yubioath-flutter linux/CMakeLists.txt at 6.0.0 and 7.0.0"),
    ("com.yubico.yubioath", DesktopFile, "GH:Yubico/yubioath-flutter linux/CMakeLists.txt on main; FH"),
    // Prompts that ask for a password or a passphrase.
    ("org.kde.polkit-kde-authentication-agent-1", DesktopFile, "KDE:plasma/polkit-kde-agent-1 CMakeLists.txt; FP44:polkit-kde"),
    ("polkit-kde-authentication-agent-1", DesktopFile, "FP44:polkit-kde: the autostart file and the program"),
    ("polkit-gnome-authentication-agent-1", DesktopFile, "DEB:policykit-1-gnome (bookworm): the autostart file and the program"),
    ("polkit-mate-authentication-agent-1", Program, "FP44:mate-polkit"),
    ("lxqt-policykit-agent", Program, "FP44:lxqt-policykit"),
    ("org.kde.kscreenlocker_greet", WindowClass, "KDE:plasma/kscreenlocker greeter/main.cpp sets the organisation domain; GH:qt/qtbase qwaylandwindow.cpp builds the app-id from it"),
    ("kscreenlocker_greet", Program, "FP44:kscreenlocker /usr/libexec/kscreenlocker_greet"),
    ("org.kde.ksshaskpass", DesktopFile, "KDE:plasma/ksshaskpass; FP44:ksshaskpass"),
    ("ksshaskpass", Program, "FP44:ksshaskpass /usr/bin/ksshaskpass"),
    ("ssh-askpass", Program, "FP44:openssh-askpass /usr/libexec/openssh/ssh-askpass; DEB:ssh-askpass"),
    ("gnome-ssh-askpass", Program, "FP44:openssh-askpass"),
    ("x11-ssh-askpass", Program, "FP44:x11-ssh-askpass"),
    ("gcr-ssh-askpass", Program, "FP44:gcr3"),
    ("gcr4-ssh-askpass", Program, "FP44:gcr"),
    ("gcr-prompter", DesktopFile, "GNOME:GNOME/gcr branch gcr-3-41 ui/gcr-prompter.desktop.in.in; FP44:gcr3"),
    ("org.gnome.keyring.SystemPrompter", WindowClass, "FP44:gcr3: a D-Bus name of the prompter, not a desktop file"),
    ("org.gnome.keyring.PrivatePrompter", WindowClass, "FP44:gcr3: a D-Bus name of the prompter, not a desktop file"),
    ("org.gnupg.pinentry-qt", DesktopFile, "GH:gpg/pinentry qt/Makefile.am, qt/main.cpp setDesktopFileName; FP44:pinentry-qt; DEB:pinentry-qt"),
    ("org.gnupg.pinentry-qt5", DesktopFile, "GH:gpg/pinentry qt5/Makefile.am, qt5/main.cpp"),
    ("pinentry", Program, "FP44:pinentry"),
    ("pinentry-qt", Program, "FP44:pinentry-qt"),
    ("pinentry-qt4", Program, "FP44:pinentry-qt"),
    ("pinentry-qt5", Program, "GH:gpg/pinentry qt5/Makefile.am"),
    ("pinentry-gnome3", Program, "GH:gpg/pinentry gnome3/Makefile.am; FP44:pinentry-gnome3"),
    ("pinentry-gtk-2", Program, "GH:gpg/pinentry gtk+-2/Makefile.am; in no Fedora 44 or Debian sid package"),
}

/// The global mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// The rules of the addendum, unchanged.
    #[default]
    Normal,
    /// Ask before every action, of every class.
    AskEveryAction,
    /// Only class 0 actions are available.
    ObserveOnly,
}

impl Mode {
    /// Every mode.
    pub const ALL: [Mode; 3] = [Mode::Normal, Mode::AskEveryAction, Mode::ObserveOnly];

    /// The mode as written in the policy file.
    pub const fn as_str(self) -> &'static str {
        match self {
            Mode::Normal => "normal",
            Mode::AskEveryAction => "ask_every_action",
            Mode::ObserveOnly => "observe_only",
        }
    }
}

/// How far a relaxation goes.
///
/// There is one level. Class 2 handling of accessibility and screen actions in
/// a named application is the only standing relaxation the design has
/// (addendum sections 5.3 and 17, item 7); there is no level for class 3 or
/// class 4, and the type has no value for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RelaxationLevel {
    /// Class 2: consequential accessibility and screen actions in the named
    /// application run without asking, in normal mode, while attended.
    Consequential,
}

impl RelaxationLevel {
    /// Every level.
    pub const ALL: [RelaxationLevel; 1] = [RelaxationLevel::Consequential];

    /// The class number written in the policy file.
    pub const fn class_number(self) -> u8 {
        match self {
            RelaxationLevel::Consequential => 2,
        }
    }
}

/// A relaxation for one named application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relaxation {
    /// The application identifier. Matched exactly, including case.
    pub application: String,
    /// The highest class the relaxation covers.
    pub level: RelaxationLevel,
}

/// The user's policy.
///
/// The default is normal mode with no exclusions, sensitive applications or
/// relaxations of the user's own. The built-in lists apply all the same.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserPolicy {
    /// The global mode.
    pub mode: Mode,
    /// Applications the assistant must neither observe nor act in, in addition
    /// to [`DEFAULT_EXCLUDED_APPLICATIONS`].
    pub excluded_applications: Vec<String>,
    /// Applications in which every action is treated as class 2 at least, in
    /// addition to [`DEFAULT_SENSITIVE_APPLICATIONS`].
    pub sensitive_applications: Vec<String>,
    /// Per-application relaxations.
    pub relaxations: Vec<Relaxation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPolicy {
    schema_version: u64,
    mode: Mode,
    #[serde(default)]
    excluded_applications: Vec<String>,
    #[serde(default)]
    sensitive_applications: Vec<String>,
    #[serde(default)]
    relaxations: Vec<RawRelaxation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRelaxation {
    application: String,
    class: i64,
}

impl UserPolicy {
    /// The policy to use when the policy file cannot be read or is invalid:
    /// observe-only.
    pub fn fail_closed() -> Self {
        UserPolicy {
            mode: Mode::ObserveOnly,
            ..UserPolicy::default()
        }
    }

    /// Load a policy from the text of a policy file.
    ///
    /// Returns every problem found, not only the first.
    pub fn from_json(text: &str) -> Result<Self, Vec<Problem>> {
        let raw: RawPolicy = serde_json::from_str(text)
            .map_err(|err| vec![Problem::new(Code::PolicyFileParse, err.to_string())])?;

        let mut problems = Vec::new();
        if raw.schema_version != POLICY_SCHEMA_VERSION {
            problems.push(Problem::new(
                Code::PolicyFileVersion,
                format!(
                    "schema_version is {}, this build reads {POLICY_SCHEMA_VERSION}",
                    raw.schema_version
                ),
            ));
        }

        let lists = [
            ("excluded_applications", &raw.excluded_applications),
            ("sensitive_applications", &raw.sensitive_applications),
        ];
        for (list, applications) in lists {
            for (index, application) in applications.iter().enumerate() {
                if !is_plain_application_id(application) {
                    problems.push(Problem::new(
                        Code::PolicyFileApplication,
                        format!("{list}[{index}]: {APPLICATION_RULE}"),
                    ));
                }
            }
        }

        let mut relaxed = BTreeSet::new();
        let mut relaxations = Vec::with_capacity(raw.relaxations.len());
        for (index, relaxation) in raw.relaxations.into_iter().enumerate() {
            if !is_plain_application_id(&relaxation.application) {
                problems.push(Problem::new(
                    Code::PolicyFileApplication,
                    format!("relaxations[{index}].application: {APPLICATION_RULE}"),
                ));
                continue;
            }
            // Two entries that differ only in the way exclusion and
            // sensitivity ignore are the same application written twice.
            if !relaxed.insert(canonical(&relaxation.application).to_ascii_lowercase()) {
                problems.push(Problem::new(
                    Code::PolicyFileRelaxation,
                    format!("relaxations[{index}]: the application already has a relaxation"),
                ));
                continue;
            }
            if relaxation.class != i64::from(RelaxationLevel::Consequential.class_number()) {
                problems.push(Problem::new(
                    Code::PolicyFileRelaxation,
                    format!(
                        "relaxations[{index}].class is {}; only class 2 can be relaxed",
                        relaxation.class
                    ),
                ));
                continue;
            }
            relaxations.push(Relaxation {
                application: relaxation.application,
                level: RelaxationLevel::Consequential,
            });
        }

        if problems.is_empty() {
            Ok(UserPolicy {
                mode: raw.mode,
                excluded_applications: raw.excluded_applications,
                sensitive_applications: raw.sensitive_applications,
                relaxations,
            })
        } else {
            Err(problems)
        }
    }

    /// Whether the application is excluded: by the user, or by
    /// [`DEFAULT_EXCLUDED_APPLICATIONS`].
    ///
    /// Compared without regard to ASCII case or surrounding whitespace, and an
    /// identifier also matches its own form with `.desktop` appended, so that
    /// the same application reported in another form is still excluded.
    pub fn is_excluded(&self, application: &str) -> bool {
        self.excluded_applications
            .iter()
            .any(|entry| same_application(entry, application))
            || DEFAULT_EXCLUDED_APPLICATIONS
                .iter()
                .any(|entry| same_application(entry, application))
    }

    /// Whether every accessibility and screen action in the application is
    /// treated as class 2 at least.
    ///
    /// True if the user marked the application sensitive, if it is one of the
    /// [`DEFAULT_SENSITIVE_APPLICATIONS`], or if its identifier is not a plain
    /// one ([`is_plain_application_id`]): such an identifier cannot be
    /// compared reliably with any list, so it is not known what the
    /// application is. Compared like [`UserPolicy::is_excluded`].
    pub fn is_sensitive(&self, application: &str) -> bool {
        !is_plain_application_id(application)
            || self
                .sensitive_applications
                .iter()
                .any(|entry| same_application(entry, application))
            || DEFAULT_SENSITIVE_APPLICATIONS
                .iter()
                .any(|entry| same_application(entry, application))
    }

    /// The relaxation for the application, if any.
    ///
    /// Compared exactly, so that a near-identical identifier is not relaxed.
    pub fn relaxation_for(&self, application: &str) -> Option<RelaxationLevel> {
        self.relaxations
            .iter()
            .filter(|relaxation| relaxation.application == application)
            .map(|relaxation| relaxation.level)
            .min()
    }
}

const APPLICATION_RULE: &str = "an application identifier must be 1 to 255 printable ASCII characters with no surrounding space";

const DESKTOP_SUFFIX: &str = ".desktop";

/// Whether an application identifier is a plain one: 1 to 255 printable ASCII
/// characters, with no space at either end.
///
/// The identifiers desktops use are of this kind. One that is not (a letter of
/// another script, a character that does not show, a control character) may
/// look the same as a listed identifier without being equal to it. The policy
/// file therefore accepts plain identifiers only, and the engine treats an
/// application whose reported identifier is not plain as sensitive.
///
/// ```
/// use tur_pax::policy::is_plain_application_id;
///
/// assert!(is_plain_application_id("org.kde.konsole"));
/// assert!(is_plain_application_id("VirtualBox Manager"));
/// assert!(!is_plain_application_id(""));
/// assert!(!is_plain_application_id(" org.kde.konsole"));
/// assert!(!is_plain_application_id("org.kde.konsole\u{200b}"));
/// ```
pub fn is_plain_application_id(application: &str) -> bool {
    !application.is_empty()
        && application.len() <= 255
        && application.bytes().all(|b| matches!(b, b' '..=b'~'))
        && !application.starts_with(' ')
        && !application.ends_with(' ')
}

/// `application` without surrounding whitespace and without a trailing
/// `.desktop`, which some toolkits append to the identifier they report. Used
/// to recognise one application written twice in the list of relaxations.
fn canonical(application: &str) -> &str {
    let trimmed = application.trim();
    let split = trimmed.len().saturating_sub(DESKTOP_SUFFIX.len());
    if split > 0
        && trimmed.is_char_boundary(split)
        && trimmed[split..].eq_ignore_ascii_case(DESKTOP_SUFFIX)
    {
        &trimmed[..split]
    } else {
        trimmed
    }
}

/// Whether two identifiers name the same application: equal without regard to
/// ASCII case and surrounding whitespace, or one is the other followed by
/// `.desktop`.
fn same_application(left: &str, right: &str) -> bool {
    let (left, right) = (left.trim(), right.trim());
    left.eq_ignore_ascii_case(right) || is_desktop_form(left, right) || is_desktop_form(right, left)
}

/// Whether `longer` is `shorter` followed by `.desktop`.
fn is_desktop_form(shorter: &str, longer: &str) -> bool {
    !shorter.is_empty()
        && longer.len() == shorter.len() + DESKTOP_SUFFIX.len()
        && longer.is_char_boundary(shorter.len())
        && longer[..shorter.len()].eq_ignore_ascii_case(shorter)
        && longer[shorter.len()..].eq_ignore_ascii_case(DESKTOP_SUFFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(result: Result<UserPolicy, Vec<Problem>>) -> Vec<Code> {
        result
            .expect_err("the policy must be rejected")
            .into_iter()
            .map(|problem| problem.code)
            .collect()
    }

    #[test]
    fn loads_a_complete_policy() {
        let policy = UserPolicy::from_json(
            r#"{
                "schema_version": 1,
                "mode": "ask_every_action",
                "excluded_applications": ["org.example.Vault"],
                "sensitive_applications": ["org.example.Bank"],
                "relaxations": [
                    { "application": "org.example.Mail", "class": 2 }
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(policy.mode, Mode::AskEveryAction);
        assert!(policy.is_excluded("org.example.Vault"));
        assert!(policy.is_sensitive("org.example.Bank"));
        assert_eq!(
            policy.relaxation_for("org.example.Mail"),
            Some(RelaxationLevel::Consequential)
        );
        assert_eq!(policy.relaxation_for("org.example.Other"), None);
    }

    #[test]
    fn lists_are_optional_but_mode_and_version_are_not() {
        let policy = UserPolicy::from_json(r#"{ "schema_version": 1, "mode": "normal" }"#).unwrap();
        assert_eq!(policy, UserPolicy::default());

        assert_eq!(
            codes(UserPolicy::from_json(r#"{ "schema_version": 1 }"#)),
            [Code::PolicyFileParse]
        );
        assert_eq!(
            codes(UserPolicy::from_json(r#"{ "mode": "normal" }"#)),
            [Code::PolicyFileParse]
        );
        assert_eq!(codes(UserPolicy::from_json("{}")), [Code::PolicyFileParse]);
        assert_eq!(codes(UserPolicy::from_json("")), [Code::PolicyFileParse]);
    }

    #[test]
    fn every_mode_round_trips_through_its_file_name() {
        for mode in Mode::ALL {
            let text = format!(r#"{{ "schema_version": 1, "mode": "{}" }}"#, mode.as_str());
            assert_eq!(UserPolicy::from_json(&text).unwrap().mode, mode);
        }
    }

    #[test]
    fn rejects_unknown_fields_modes_and_versions() {
        assert_eq!(
            codes(UserPolicy::from_json(
                r#"{ "schema_version": 1, "mode": "normal", "allow_everything": true }"#
            )),
            [Code::PolicyFileParse]
        );
        for mode in ["relaxed", "Normal", "NORMAL", " normal", "normal ", ""] {
            let text = format!(r#"{{ "schema_version": 1, "mode": "{mode}" }}"#);
            assert_eq!(
                codes(UserPolicy::from_json(&text)),
                [Code::PolicyFileParse],
                "{mode:?}"
            );
        }
        for version in ["0", "2", "18446744073709551615"] {
            let text = format!(r#"{{ "schema_version": {version}, "mode": "normal" }}"#);
            assert_eq!(
                codes(UserPolicy::from_json(&text)),
                [Code::PolicyFileVersion],
                "{version}"
            );
        }
        for version in ["-1", "1.0", "\"1\"", "null", "true", "[1]"] {
            let text = format!(r#"{{ "schema_version": {version}, "mode": "normal" }}"#);
            assert_eq!(
                codes(UserPolicy::from_json(&text)),
                [Code::PolicyFileParse],
                "{version}"
            );
        }
        assert_eq!(
            codes(UserPolicy::from_json("not json")),
            [Code::PolicyFileParse]
        );
    }

    #[test]
    fn a_repeated_field_is_an_error_and_never_the_last_or_the_first_value() {
        for text in [
            r#"{ "schema_version": 1, "mode": "observe_only", "mode": "normal" }"#,
            r#"{ "schema_version": 1, "mode": "normal", "mode": "observe_only" }"#,
            r#"{ "schema_version": 2, "schema_version": 1, "mode": "normal" }"#,
            r#"{ "schema_version": 1, "mode": "normal",
                 "excluded_applications": ["org.example.Vault"], "excluded_applications": [] }"#,
            r#"{ "schema_version": 1, "mode": "normal",
                 "sensitive_applications": ["org.example.Bank"], "sensitive_applications": [] }"#,
            r#"{ "schema_version": 1, "mode": "normal", "relaxations": [], "relaxations": [] }"#,
            r#"{ "schema_version": 1, "mode": "normal", "relaxations": [
                 { "application": "org.example.Mail", "class": 3, "class": 2 } ] }"#,
            r#"{ "schema_version": 1, "mode": "normal", "relaxations": [
                 { "application": "org.example.Bank", "application": "org.example.Mail", "class": 2 } ] }"#,
        ] {
            assert_eq!(
                codes(UserPolicy::from_json(text)),
                [Code::PolicyFileParse],
                "{text}"
            );
        }
    }

    #[test]
    fn only_class_2_can_be_relaxed() {
        for class in ["-1", "0", "1", "3", "4", "5", "9223372036854775807"] {
            let text = format!(
                r#"{{ "schema_version": 1, "mode": "normal",
                     "relaxations": [{{ "application": "org.example.App", "class": {class} }}] }}"#
            );
            assert_eq!(
                codes(UserPolicy::from_json(&text)),
                [Code::PolicyFileRelaxation],
                "class {class}"
            );
        }
        for class in ["2.0", "\"2\"", "null", "[2]", "9223372036854775808"] {
            let text = format!(
                r#"{{ "schema_version": 1, "mode": "normal",
                     "relaxations": [{{ "application": "org.example.App", "class": {class} }}] }}"#
            );
            assert_eq!(
                codes(UserPolicy::from_json(&text)),
                [Code::PolicyFileParse],
                "class {class}"
            );
        }
        assert_eq!(RelaxationLevel::ALL.len(), 1);
        assert_eq!(RelaxationLevel::Consequential.class_number(), 2);
    }

    #[test]
    fn rejects_duplicate_relaxations_and_bad_application_names() {
        for second in [
            "org.example.App",
            "org.example.app",
            "ORG.EXAMPLE.APP",
            "org.example.App.desktop",
        ] {
            let text = format!(
                r#"{{ "schema_version": 1, "mode": "normal", "relaxations": [
                    {{ "application": "org.example.App", "class": 2 }},
                    {{ "application": "{second}", "class": 2 }}
                ] }}"#
            );
            assert_eq!(
                codes(UserPolicy::from_json(&text)),
                [Code::PolicyFileRelaxation],
                "{second}"
            );
        }
        assert_eq!(
            codes(UserPolicy::from_json(
                r#"{ "schema_version": 1, "mode": "normal",
                     "excluded_applications": ["", " padded "],
                     "sensitive_applications": ["line\nbreak"],
                     "relaxations": [{ "application": "", "class": 2 }] }"#
            )),
            [
                Code::PolicyFileApplication,
                Code::PolicyFileApplication,
                Code::PolicyFileApplication,
                Code::PolicyFileApplication
            ]
        );
    }

    #[test]
    fn an_identifier_that_is_not_plain_is_rejected_in_every_list() {
        let long = "a".repeat(256);
        for application in [
            "org.example.Vault\\u200b",
            "\\ufefforg.example.Vault",
            "org.ex\\u0430mple.Vault",
            "org.example.V\\u00e4ult",
            "org.example.Vault\\u00a0",
            "org.example.Vault\\t",
            "org.example.Vault\\u007f",
            "org.example\\u202e.Vault",
            long.as_str(),
        ] {
            for list in ["excluded_applications", "sensitive_applications"] {
                let text = format!(
                    r#"{{ "schema_version": 1, "mode": "normal", "{list}": ["{application}"] }}"#
                );
                assert_eq!(
                    codes(UserPolicy::from_json(&text)),
                    [Code::PolicyFileApplication],
                    "{list}: {application}"
                );
            }
            let text = format!(
                r#"{{ "schema_version": 1, "mode": "normal",
                     "relaxations": [{{ "application": "{application}", "class": 2 }}] }}"#
            );
            assert_eq!(
                codes(UserPolicy::from_json(&text)),
                [Code::PolicyFileApplication],
                "relaxations: {application}"
            );
        }
        // The longest identifier, and one with a space inside, are accepted.
        let longest = "a".repeat(255);
        for application in [longest.as_str(), "VirtualBox Manager", "a"] {
            let text = format!(
                r#"{{ "schema_version": 1, "mode": "normal", "excluded_applications": ["{application}"] }}"#
            );
            assert!(UserPolicy::from_json(&text).is_ok(), "{application}");
        }
    }

    #[test]
    fn exclusion_matches_loosely_and_relaxation_exactly() {
        let policy = UserPolicy {
            mode: Mode::Normal,
            excluded_applications: vec!["org.example.Vault".to_owned()],
            sensitive_applications: vec!["org.example.Bank".to_owned()],
            relaxations: vec![Relaxation {
                application: "org.example.Mail".to_owned(),
                level: RelaxationLevel::Consequential,
            }],
        };
        for application in [
            "org.example.Vault",
            "ORG.EXAMPLE.VAULT",
            " org.example.vault ",
            "\torg.example.Vault\n",
            "org.example.Vault.desktop",
            "org.example.vault.DESKTOP",
            " org.example.Vault.desktop ",
        ] {
            assert!(policy.is_excluded(application), "{application:?}");
        }
        for application in [
            "org.example.Vault2",
            "org.example.Vaul",
            "org.example",
            "xorg.example.Vault",
            "org.example.Vault.desktop.desktop",
            "org.example.Vault.desktopx",
            "org.example.Vault desktop",
            ".desktop",
            "",
        ] {
            assert!(!policy.is_excluded(application), "{application:?}");
        }
        assert!(policy.is_sensitive("org.example.bank"));
        assert!(policy.is_sensitive("org.example.Bank.desktop"));
        assert!(!policy.is_sensitive("org.example.Banks"));

        assert_eq!(policy.relaxation_for("org.example.mail"), None);
        assert_eq!(policy.relaxation_for(" org.example.Mail"), None);
        assert_eq!(policy.relaxation_for("org.example.Mail.desktop"), None);
    }

    #[test]
    fn an_application_that_cannot_be_identified_is_sensitive() {
        let policy = UserPolicy::default();
        for application in [
            "",
            " org.example.Editor",
            "org.example.Editor ",
            "org.example.Editor\u{200b}",
            "org.ex\u{430}mple.Editor",
            "org.example.Edit\u{f6}r",
            "org.example.Editor\n",
            "\u{feff}org.example.Editor",
        ] {
            assert!(policy.is_sensitive(application), "{application:?}");
        }
        let too_long = "a".repeat(256);
        assert!(policy.is_sensitive(&too_long));
        assert!(!policy.is_sensitive("org.example.Editor"));
        assert!(!policy.is_sensitive(&"a".repeat(255)));
    }

    #[test]
    fn the_built_in_lists_apply_to_every_policy() {
        for policy in [UserPolicy::default(), UserPolicy::fail_closed()] {
            for application in DEFAULT_SENSITIVE_APPLICATIONS {
                assert!(policy.is_sensitive(application), "{application}");
                assert!(
                    policy.is_sensitive(&application.to_ascii_uppercase()),
                    "{application}"
                );
                assert!(
                    policy.is_sensitive(&format!("{application}.desktop")),
                    "{application}"
                );
            }
            for application in DEFAULT_EXCLUDED_APPLICATIONS {
                assert!(policy.is_excluded(application), "{application}");
                assert!(
                    policy.is_excluded(&application.to_ascii_lowercase()),
                    "{application}"
                );
                // Also an entry that itself ends in ".desktop".
                assert!(
                    policy.is_excluded(&format!("{application}.desktop")),
                    "{application}"
                );
            }
            assert!(!policy.is_sensitive("org.kde.kate"));
            assert!(!policy.is_excluded("org.kde.kate"));
            // An entry is a whole identifier, not the beginning of one.
            assert!(!policy.is_sensitive("stellarium"));
            assert!(!policy.is_sensitive("org.kde.konsolepart"));
        }
    }

    #[test]
    fn the_built_in_lists_are_plain_and_free_of_duplicates() {
        for list in [
            DEFAULT_SENSITIVE_APPLICATIONS,
            DEFAULT_EXCLUDED_APPLICATIONS,
        ] {
            let mut seen = BTreeSet::new();
            for application in list {
                assert!(is_plain_application_id(application), "{application}");
                assert!(
                    seen.insert(application.to_ascii_lowercase()),
                    "{application} is listed twice"
                );
            }
        }
        // The two lists do not overlap: an excluded application is refused
        // before its sensitivity is looked at.
        for application in DEFAULT_SENSITIVE_APPLICATIONS {
            assert!(
                !DEFAULT_EXCLUDED_APPLICATIONS
                    .iter()
                    .any(|entry| same_application(entry, application)),
                "{application}"
            );
        }
        // The surfaces named in the design.
        for application in ["org.kde.konsole", "org.kde.krunner", "org.gnome.Terminal"] {
            assert!(DEFAULT_SENSITIVE_APPLICATIONS.contains(&application));
        }
    }

    #[test]
    fn every_built_in_identifier_has_a_source() {
        for (list, table) in [
            (
                DEFAULT_SENSITIVE_APPLICATIONS,
                SENSITIVE_APPLICATION_SOURCES,
            ),
            (DEFAULT_EXCLUDED_APPLICATIONS, EXCLUDED_APPLICATION_SOURCES),
        ] {
            // The table is the list, row for row.
            assert_eq!(list.len(), table.len());
            for (id, row) in list.iter().zip(table) {
                assert_eq!(*id, row.id);
                assert!(!row.source.trim().is_empty(), "{id} has no source");
            }
        }
        // What could not be confirmed is known by name. An entry added to a
        // list without a source that shows it has to be added here too.
        let unverified: Vec<&str> = SENSITIVE_APPLICATION_SOURCES
            .iter()
            .chain(EXCLUDED_APPLICATION_SOURCES)
            .filter(|row| row.evidence == Evidence::Unverified)
            .map(|row| row.id)
            .collect();
        assert_eq!(
            unverified,
            [
                "org.codeberg.dnkl.foot",
                "hyper",
                "cmd.exe",
                "powershell.exe",
                "pwsh.exe",
                "conhost.exe",
                "wineconsole",
                "wineconsole.exe",
                "org.keepass.KeePass",
                "com.1password.1Password",
                "1password",
                "proton-pass",
                "com.enpass.Enpass",
                "org.qtpass.QtPass",
            ]
        );
    }

    #[test]
    fn the_identifiers_upstream_really_uses_are_covered() {
        // Identifiers as the applications' own sources and packages have
        // them, checked on 2026-11-12. Each of these was missing from the
        // list, or the list had another spelling, before that check.
        let policy = UserPolicy::default();
        for application in [
            "com.onepassword.OnePassword",
            "com.onepassword.OnePassword.desktop",
            "org.kde.kwalletd",
            "kwalletmanager5-kwalletd",
            "org.gnome.Seahorse",
            "org.gnupg.pinentry-qt",
            "org.gnupg.pinentry-qt5",
            "org.gnome.keyring.PrivatePrompter",
            "gcr4-ssh-askpass",
            "gnome-ssh-askpass",
            "keepass",
            "org.ijhack.qtpass",
            "Proton Pass",
            "polkit-mate-authentication-agent-1",
        ] {
            assert!(policy.is_excluded(application), "{application}");
        }
        for application in [
            "org.contourterminal.Contour",
            "foot-server",
            "dev.warp.WarpPreview",
            "com.mitchellh.ghostty-debug",
            "alacritty",
            "Rio",
            "com.rioterm.Rio",
            "rxvt-unicode",
            "org.gnome.Ptyxis.Devel",
            "org.gnome.Console.Devel",
            "debian-xterm",
            "qterminal-drop",
            "xfce4-run",
        ] {
            assert!(policy.is_sensitive(application), "{application}");
            assert!(!policy.is_excluded(application), "{application}");
        }
        // The identifiers the design names, in the forms a compositor may
        // report them.
        for application in [
            "org.kde.konsole",
            "org.kde.konsole.desktop",
            "org.kde.yakuake",
            "org.gnome.Terminal",
            "org.gnome.Ptyxis",
            "app.devsuite.Ptyxis",
            "org.kde.krunner",
        ] {
            assert!(policy.is_sensitive(application), "{application}");
        }
        for application in [
            "org.keepassxc.KeePassXC",
            "com.bitwarden.desktop",
            "org.kde.kwalletmanager",
            "org.kde.kwalletmanager5",
            "org.gnome.seahorse.Application",
        ] {
            assert!(policy.is_excluded(application), "{application}");
        }
    }

    #[test]
    fn a_policy_built_in_code_may_name_an_application_twice() {
        let policy = UserPolicy {
            relaxations: vec![
                Relaxation {
                    application: "org.example.Mail".to_owned(),
                    level: RelaxationLevel::Consequential,
                },
                Relaxation {
                    application: "org.example.Mail".to_owned(),
                    level: RelaxationLevel::Consequential,
                },
            ],
            ..UserPolicy::default()
        };
        assert_eq!(
            policy.relaxation_for("org.example.Mail"),
            Some(RelaxationLevel::Consequential)
        );
    }

    #[test]
    fn fail_closed_is_observe_only() {
        assert_eq!(UserPolicy::fail_closed().mode, Mode::ObserveOnly);
    }
}
