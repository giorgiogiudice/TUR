// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit

// What could not be done or read, as the kit's two-layer message
// (desktop-experience.md, 5.6). The content is a failure as src/codes.h
// builds it: one of this program's codes, what happened, why, and the
// technical layer.
Kit.Message {
    id: message

    // code, title, reason, detail.
    required property var failure

    // Offer to put the message away. Not for one that stays true for as long
    // as the page is shown.
    property bool dismissible: false

    signal dismissed()

    code: message.failure.code !== undefined ? message.failure.code : ""
    title: message.failure.title !== undefined ? message.failure.title : ""
    reason: message.failure.reason !== undefined ? message.failure.reason : ""
    technicalDetail: message.failure.detail !== undefined ? message.failure.detail : ""
    actions: message.dismissible ? [qsTr("Close")] : []

    // The program that makes a support bundle is a later task (D-08). No
    // button is shown that would do nothing.
    supportBundleAvailable: false

    onActionTriggered: message.dismissed()
}
