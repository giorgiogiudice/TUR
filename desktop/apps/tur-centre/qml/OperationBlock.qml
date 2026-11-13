// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// Where an installation, update or removal shows itself on an application's
// page (desktop-experience.md, 8.5): progress replaces the button and the
// page stays usable.
//
//     Downloading and installing                          [Cancel]
//     ▓▓▓▓▓▓▓▓▓░░░░░░░░░░░░░░░░░   (a bar with numbers only when the
//     Installing runtime/org.freedesktop.Platform/...      total is known)
//
// Cancel is offered only while tur-apps can still take everything back.
// The end is said in one line; a failure is the two-layer message with its
// code, and the page is as it was before the attempt.
ColumnLayout {
    id: block

    property AppsOperation operation: null
    // The application's name, for the sentences.
    property string name

    // The failure says a source is switched off: offer to switch it on.
    signal switchOnRequested(string source)
    // The failure says the installer must come from the person.
    signal installerRequested()

    readonly property int phase: block.operation ? block.operation.phase : -1
    readonly property string route: block.operation && block.operation.plan.route ? block.operation.plan.route : ""
    readonly property bool measured: block.operation !== null && block.operation.total > 0
    readonly property string failureCode: block.operation && block.operation.failure.code
                                          ? block.operation.failure.code : ""

    visible: block.operation !== null && block.phase !== AppsOperation.Planned
    spacing: Kit.Theme.spaceRelated

    Kit.BusyIndicator {
        objectName: "operationPlanning"
        visible: block.phase === AppsOperation.Planning
        running: visible
        label: qsTr("Making the plan: where it comes from, how large it is, what it may reach")
    }

    ColumnLayout {
        objectName: "operationRunning"
        Layout.fillWidth: true
        visible: block.phase === AppsOperation.Running
        spacing: Kit.Theme.spaceGrid

        RowLayout {
            Layout.fillWidth: true
            spacing: Kit.Theme.spaceGroup

            Kit.ProgressBar {
                id: progress

                objectName: "operationProgress"
                Layout.fillWidth: true
                label: block.operation ? Words.stepLabel(block.operation.kind, block.operation.step, block.route) : ""
                //: Real figures of a download: %1 done, %2 in all, for example "310 MB of 1.1 GB".
                detail: block.measured ? qsTr("%1 of %2").arg(Words.size(block.operation.done))
                                                         .arg(Words.size(block.operation.total)) : ""
                indeterminate: !block.measured
                from: 0
                to: block.measured ? block.operation.total : 1
                value: block.measured ? block.operation.done : 0
            }

            Kit.Button {
                objectName: "operationCancel"
                Layout.alignment: Qt.AlignBottom
                visible: block.operation !== null && (block.operation.cancellable || block.operation.cancelRequested)
                enabled: block.operation !== null && block.operation.cancellable
                text: block.operation && block.operation.cancelRequested ? qsTr("Cancelling…") : qsTr("Cancel")
                onClicked: block.operation.cancel()
            }
        }

        // What tur-apps says is happening, as it says it.
        Kit.Label {
            objectName: "operationActivity"
            Layout.fillWidth: true
            visible: text.length > 0
            text: block.operation ? block.operation.activity : ""
            kind: Kit.Label.Small
            tone: Kit.Label.Secondary
            // A reference such as runtime/org.freedesktop.Platform/x86_64/25.08
            // has no spaces to wrap at; nothing is cut off.
            wrapMode: Text.WrapAnywhere
        }
    }

    Kit.Banner {
        objectName: "operationDone"
        Layout.fillWidth: true
        visible: block.phase === AppsOperation.Succeeded || block.phase === AppsOperation.Cancelled
        severity: block.phase === AppsOperation.Cancelled ? Kit.Banner.Info : Kit.Banner.Positive
        text: {
            const operation = block.operation
            if (!operation)
                return ""
            if (block.phase === AppsOperation.Cancelled)
                return qsTr("Cancelled. Nothing was installed.")
            switch (operation.kind) {
            case AppsOperation.Install:
                //: %1 is the application's name.
                return qsTr("%1 is installed.").arg(block.name)
            case AppsOperation.Update:
                if (operation.result.updated === true)
                    //: %1 is the application's name.
                    return qsTr("%1 is updated.").arg(block.name)
                //: %1 is the application's name.
                return qsTr("%1 was already up to date.").arg(block.name)
            case AppsOperation.Remove:
                if (operation.deleteData)
                    //: %1 is the application's name.
                    return qsTr("%1 is removed, with its settings and data.").arg(block.name)
                //: %1 is the application's name.
                return qsTr("%1 is removed. Its settings and documents were kept.").arg(block.name)
            }
            return ""
        }
        actionText: qsTr("Close")
        onTriggered: block.operation.dismiss()
    }

    Loader {
        objectName: "operationFailure"
        Layout.fillWidth: true
        active: block.phase === AppsOperation.Failed
        visible: active

        sourceComponent: ColumnLayout {
            spacing: Kit.Theme.spaceRelated

            FailureMessage {
                Layout.fillWidth: true
                failure: block.operation.failure
                onRetryRequested: block.operation.retry()
            }

            Flow {
                Layout.fillWidth: true
                spacing: Kit.Theme.spaceRelated

                Kit.Button {
                    objectName: "operationSwitchOn"
                    // tur-apps: "The source is not switched on".
                    visible: block.failureCode === "TUR-DESKTOP-APPS-008"
                    text: qsTr("Switch on Flathub and try again")
                    kind: Kit.Button.Primary
                    onClicked: block.switchOnRequested("flathub")
                }

                Kit.Button {
                    objectName: "operationChooseInstaller"
                    visible: block.failureCode === "TUR-DESKTOP-APPS-005"
                    text: qsTr("Choose the installer…")
                    onClicked: block.installerRequested()
                }

                Kit.Button {
                    objectName: "operationClose"
                    text: qsTr("Close")
                    kind: Kit.Button.Quiet
                    onClicked: block.operation.dismiss()
                }
            }
        }
    }
}
