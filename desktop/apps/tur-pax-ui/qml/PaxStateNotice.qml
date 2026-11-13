// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import Tur.Kit as Kit

// What P@x's window says when P@x cannot take a task (desktop-experience.md,
// 9.1): the service is not there (a plain message, nothing to set up); P@x
// is off and was never set up (the design's small card, with "Set up P@x");
// or it is on, or was, and something is missing (the service's own reason
// and code, with "Set up P@x"). TUR never reminds anyone that P@x is off:
// this shows only in P@x's own window, which the person opened.
ColumnLayout {
    id: notice

    required property AgentClient client

    // absent, looking, off, switched-off, not-ready or ready.
    readonly property string situation: {
        const client = notice.client
        if (client.presence === AgentClient.Absent)
            return "absent"
        if (client.presence !== AgentClient.Present || !client.statusKnown)
            return "looking"
        const status = client.status
        if (status.ready === true)
            return "ready"
        const consent = status.consent || {}
        if (status.enabled !== true)
            return consent.accepted_version === null || consent.accepted_version === undefined ? "off" : "switched-off"
        return "not-ready"
    }
    readonly property bool blocking: notice.situation !== "ready"
    readonly property var problem: notice.client.status.problem || null

    signal setupRequested()
    signal dismissed()

    objectName: "paxState-" + notice.situation
    visible: notice.blocking
    spacing: Kit.Theme.spaceRelated

    Accessible.role: Accessible.AlertMessage
    Accessible.name: title.text

    Kit.Label {
        id: title

        objectName: "stateTitle"
        Layout.fillWidth: true
        text: {
            switch (notice.situation) {
            case "absent":
                return Words.absentTitle
            case "looking":
                return Words.looking
            case "off":
                return Words.offTitle
            case "switched-off":
                return Words.switchedOffTitle
            default:
                return Words.notReadyTitle
            }
        }
        kind: Kit.Label.Heading
        wrapMode: Text.Wrap
    }

    Kit.Label {
        objectName: "stateText"
        Layout.fillWidth: true
        visible: text.length > 0
        text: {
            switch (notice.situation) {
            case "absent":
                return Words.absentText + (notice.client.presenceDetail.length > 0 ? " " + notice.client.presenceDetail : "")
            case "off":
                return Words.offText
            case "switched-off":
            case "not-ready": {
                if (notice.problem)
                    return String(notice.problem.message || "")
                const status = notice.client.status
                if (status.key === "not_set")
                    return Words.noKey(String(status.provider_name || status.provider || ""))
                if (status.key === "file_missing")
                    return Words.localNotRunning
                return ""
            }
            default:
                return ""
            }
        }
        paragraph: true
        wrapMode: Text.Wrap
    }

    Kit.CodeLabel {
        objectName: "stateCode"
        visible: (notice.situation === "not-ready" || notice.situation === "switched-off") && code.length > 0
        code: notice.problem ? String(notice.problem.error_code || "") : ""
    }

    Flow {
        Layout.fillWidth: true
        Layout.topMargin: Kit.Theme.spaceGrid
        visible: notice.situation !== "absent" && notice.situation !== "looking"
        spacing: Kit.Theme.spaceRelated

        Kit.Button {
            objectName: "setupButton"
            text: Words.setup
            kind: Kit.Button.Primary
            onClicked: notice.setupRequested()
        }

        Kit.Button {
            objectName: "notNowButton"
            text: Words.notNow
            onClicked: notice.dismissed()
        }
    }
}
