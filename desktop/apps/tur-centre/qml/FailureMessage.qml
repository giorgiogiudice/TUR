// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Tur.Kit as Kit

// What could not be done, as the kit's two-layer message
// (desktop-experience.md, 5.6). The content is a failure as src/answers.h
// builds it: the catalogue's own code, title and message where the catalogue
// reported the failure, and one of this program's codes where the asking
// itself failed. The command and how it ended are in the technical layer.
Kit.Message {
    id: message

    // code, title, reason, detail, retryable.
    required property var failure

    signal retryRequested()

    severity: Kit.Message.Error
    code: message.failure.code ?? ""
    title: message.failure.title ?? ""
    reason: message.failure.reason ?? ""
    technicalDetail: message.failure.detail ?? ""
    actions: message.failure.retryable ? [qsTr("Try again")] : []

    // The program that makes a support bundle is a later task (D-08). No
    // button is shown that would do nothing.
    supportBundleAvailable: false

    onActionTriggered: message.retryRequested()
}
