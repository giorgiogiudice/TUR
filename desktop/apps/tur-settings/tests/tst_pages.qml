// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
import QtQuick
import QtTest
import Tur.Settings
import Tur.Kit as Kit
import Tur.Kit.Testing as Testing

// The three pages (desktop-experience.md, 10.2): what each shows, where it
// reads it from, and what it says about the things it cannot read yet.
Item {
    id: root

    width: 1080
    height: 2400

    SettingsStage {
        id: host

        anchors.fill: parent
    }

    TestCase {
        name: "Pages"
        when: windowShown

        function initTestCase() {
            Kit.Motion.frozen = true
        }

        function cleanup() {
            host.stop()
            Kit.Theme.light = false
            settingsHelperRecord.forget()
            wait(0)
        }

        // Starts on a fixture set, at a page; returns the page.
        function page(set, address, properties) {
            const shell = host.start(set, properties)
            verify(host.service.go(address).ok, address + " opens")
            const view = findChild(shell, "shellView").item
            compare(view.address, address)
            return view
        }

        function row(view, address) {
            const found = host.service.findRow(view, address)
            verify(found !== null, "the page shows " + address)
            return found
        }

        function helperArguments() {
            let seen
            tryVerify(() => {
                seen = Testing.Harness.readJson(settingsHelperOut)
                return seen !== undefined
            }, 10000, "the stand-in for System Settings was started")
            return seen
        }

        // --- Overview --------------------------------------------------------

        function test_overview_shows_what_the_system_says_it_is() {
            const view = page("default", "tur.overview")
            compare(findChild(view, "pageTitle").text, "Overview")
            verify(!findChild(view, "factsFailure").visible)

            const version = row(view, "tur.overview#version")
            compare(version.title, "Version")
            compare(version.value, "0.1.0-dev")
            verify(version.available)
            const build = row(view, "tur.overview#build")
            compare(build.title, "Build")
            compare(build.value, "20261112.g387ac4f62f6c")
        }

        // The state of updates, the last start, and what needs attention are
        // reported by services that do not exist yet. The rows are there, say
        // so, and give no value: least of all a reassuring one.
        function test_overview_does_not_make_up_what_it_cannot_read_data() {
            return [
                { tag: "updates", address: "tur.overview#update-state", title: "Updates",
                  reason: "TUR cannot say here whether an update is waiting: its update service is not part of the system yet." },
                { tag: "the last start", address: "tur.overview#last-start", title: "The last start",
                  reason: "TUR does not show here yet how the last start went: this page cannot read the record of each start." },
                { tag: "needs attention", address: "tur.overview#attention", title: "Needs attention",
                  reason: "TUR cannot say here whether anything needs attention: the services that would report it are not part of the system yet. That nothing is listed does not mean that nothing is wrong." }
            ]
        }

        function test_overview_does_not_make_up_what_it_cannot_read(data) {
            const view = page("default", "tur.overview")
            const item = row(view, data.address)
            compare(item.title, data.title)
            verify(!item.available)
            compare(item.value, "Not available yet")
            compare(item.unavailableReason, data.reason)
            const reason = findChild(item, "rowUnavailable")
            verify(reason.visible)
            compare(reason.text, data.reason)
            // The registry says the same: what backs the row is not there.
            compare(Registry.entry(data.address).sourceAvailable, false)
            verify(Registry.entry(data.address).waitsFor.length > 0)
        }

        function test_where_there_is_no_tur_the_pages_say_so_data() {
            return [{ tag: "overview", address: "tur.overview" }, { tag: "about", address: "tur.about" }]
        }

        function test_where_there_is_no_tur_the_pages_say_so(data) {
            const view = page("not-tur", data.address)
            const message = findChild(view, "factsFailure")
            verify(message.visible)
            compare(message.code, "TUR-DESKTOP-SETTINGS-010")
            compare(message.title, "TUR could not read what this system is.")
            compare(message.reason, "The file that records TUR's version and build could not be read. Either this is "
                    + "not a TUR system, or the system is damaged.")
            // The file that is missing, and what the system does call itself.
            verify(message.technicalDetail.indexOf(settingsFixtures + "/not-tur/tur-release") >= 0)
            verify(message.technicalDetail.indexOf("ID=fedora") >= 0)

            // Nothing is put in the place of what was not read.
            const version = row(view, data.address + "#version")
            compare(version.value, "Not recorded")
            compare(row(view, data.address + "#build").value, "Not recorded")
        }

        // --- Appearance ------------------------------------------------------

        function test_appearance_says_what_the_desktop_is_set_to() {
            const view = page("default", "tur.appearance")
            compare(findChild(view, "pageTitle").text, "Appearance")
            const current = row(view, "tur.appearance#current")
            compare(current.title, "The desktop is set to")
            // The tests run where no desktop states a preference, and TUR is
            // dark unless the desktop says light.
            compare(Kit.SystemAppearance.prefersLight, false)
            compare(current.value, "Dark")
            compare(current.detail, "")

            // A window started in a variant of its own choosing does not
            // show what the desktop is set to, and says so.
            Kit.Theme.light = true
            compare(current.value, "Dark")
            compare(current.detail, "This window was started in the light variant and does not follow the desktop.")
        }

        function test_the_choice_of_variant_is_shown_and_cannot_be_used() {
            const view = page("default", "tur.appearance")
            const variant = row(view, "tur.appearance#variant")
            compare(variant.title, "Dark, light, or light by day and dark at night")
            compare(variant.kind, "choice")
            verify(!variant.available)
            compare(variant.unavailableReason,
                    "TUR cannot change this yet: its dark and light themes are not part of the system. Until they are, "
                    + "choose a global theme or colours in System Settings, below.")
            const options = [["variantDark", "Dark"], ["variantLight", "Light"],
                             ["variantByDay", "Light by day, dark at night"]]
            for (const [name, text] of options) {
                const option = findChild(variant, name)
                verify(option !== null && option.visible, text + " is shown")
                compare(option.text, text)
                verify(!option.enabled, text + " cannot be chosen")
                // None is marked: which of the three applies cannot be read.
                verify(!option.checked)
                // A press does nothing.
                mouseClick(option)
                verify(!option.checked)
            }
        }

        function test_the_rest_opens_system_settings_at_the_page_data() {
            return [
                { tag: "global theme", address: "tur.appearance#global-theme", title: "Global theme",
                  module: "kcm_lookandfeel" },
                { tag: "colours", address: "tur.appearance#colours", title: "Colours", module: "kcm_colors" },
                { tag: "fonts", address: "tur.appearance#fonts", title: "Fonts", module: "kcm_fonts" },
                { tag: "night light", address: "tur.appearance#night-light", title: "Night Light",
                  module: "kcm_nightlight" }
            ]
        }

        function test_the_rest_opens_system_settings_at_the_page(data) {
            const view = page("default", "tur.appearance", { systemSettingsProgram: settingsHelper })
            const link = row(view, data.address)
            compare(link.title, data.title)
            compare(link.kind, "action")
            const button = findChild(link, "rowOpen")
            compare(button.text, "Open in System Settings")
            // A screen reader hears where the button leads.
            compare(Testing.Harness.accessible(button).name, "Open " + data.title + " in System Settings")
            mouseClick(button)
            compare(helperArguments(), [data.module])
            verify(!findChild(host.shell, "shellFailure").visible)
            // Nothing else happened: the page is where it was.
            compare(host.shell.address, "tur.appearance")
        }

        function test_a_link_says_when_system_settings_is_not_there() {
            const view = page("default", "tur.appearance")
            const button = findChild(row(view, "tur.appearance#colours"), "rowOpen")
            button.forceActiveFocus(Qt.TabFocusReason)
            keyClick(Qt.Key_Space)
            const message = findChild(host.shell, "shellFailure")
            verify(message.visible)
            compare(message.code, "TUR-DESKTOP-SETTINGS-005")
            verify(message.technicalDetail.indexOf("kcm_colors") >= 0)
        }

        // --- About -----------------------------------------------------------

        function test_about_shows_version_build_and_base_data() {
            return [
                { tag: "today's image", set: "default", version: "0.1.0-dev", build: "20261112.g387ac4f62f6c",
                  base: "45.20261112.0", revision: "387ac4f62f6cc07a6d0a3430c64634ca2b72fc02",
                  name: "TUR 0.1.0-dev (based on Fedora Linux 45.20261112.0 (Kinoite))", id: "ID=fedora" },
                { tag: "once the marks are replaced", set: "rebranded", version: "0.4.0",
                  build: "20270301.g0123456789ab", base: "45.20270301.0",
                  revision: "0123456789abcdef0123456789abcdef01234567", name: "TUR 0.4.0", id: "ID=tur" }
            ]
        }

        function test_about_shows_version_build_and_base(data) {
            const view = page(data.set, "tur.about")
            compare(findChild(view, "pageTitle").text, "About")
            verify(!findChild(view, "factsFailure").visible)
            compare(row(view, "tur.about#version").value, data.version)
            compare(row(view, "tur.about#build").value, data.build)
            const base = row(view, "tur.about#base")
            compare(base.title, "Base")
            compare(base.value, data.base)

            // The identifiers a person quotes when asking for help are in
            // the second layer, closed until asked for.
            const technical = findChild(view, "technicalGroup")
            verify(technical.collapsible)
            verify(!technical.expanded)
            verify(!findChild(technical, "groupCard").visible)
            mouseClick(findChild(technical, "groupOpener"))
            verify(technical.expanded)
            verify(findChild(technical, "groupCard").visible)
            compare(row(view, "tur.about#source-revision").value, data.revision)
            const image = row(view, "tur.about#base-image")
            compare(image.value, "quay.io/fedora/fedora-kinoite")
            verify(image.detail.indexOf("sha256:") === 0)
            const name = row(view, "tur.about#system-name")
            compare(name.title, "What the system calls itself")
            compare(name.value, data.name)
            verify(name.detail.indexOf(data.id) === 0)
            const program = row(view, "tur.about#settings-version")
            verify(program.value.indexOf("tur-settings ") === 0)
            verify(program.detail.indexOf("Qt 6.") === 0)
        }

        // The statement about Fedora (12.2), word for word.
        function test_about_states_where_tur_comes_from() {
            const view = page("default", "tur.about")
            const origin = row(view, "tur.about#origin")
            compare(origin.title, "TUR and the Fedora Project")
            compare(origin.detail,
                    "TUR is built from software packaged by the Fedora Project and many other projects. TUR is not "
                    + "provided, endorsed or supported by the Fedora Project or by Red Hat. Official Fedora software is "
                    + "available at https://fedoraproject.org/.")
            verify(findChild(origin, "rowDetail").visible)
        }

        function test_about_shows_the_licence_as_it_is() {
            const view = page("default", "tur.about")
            const licence = row(view, "tur.about#licence")
            compare(licence.title, "TUR's licence")
            compare(licence.value, "TUR Noncommercial License 1.0.0")
            // The notice the licence requires to be passed on.
            compare(licence.detail,
                    "Required Notice: Copyright (c) 2026 TUR project authors (https://github.com/giorgiogiudice/TUR)")

            const text = findChild(licence, "licenceText")
            verify(!text.visible, "the text is closed until asked for")
            mouseClick(findChild(licence, "licenceOpener"))
            verify(text.visible)
            // The file as shipped: its first line and one of its headings.
            verify(text.text.indexOf("# TUR Noncommercial License 1.0.0") === 0)
            verify(text.text.indexOf("## Notices") > 0)
            verify(text.text.length > 4000)

            const others = row(view, "tur.about#third-party")
            compare(others.title, "Software from other projects")
            const notices = findChild(others, "noticesText")
            verify(!notices.visible)
            mouseClick(findChild(others, "noticesOpener"))
            verify(notices.visible)
            verify(notices.text.indexOf("# Third-party notices") === 0)
            verify(notices.text.indexOf("TUR does not relicense any of these components.") > 0)
        }
    }
}
