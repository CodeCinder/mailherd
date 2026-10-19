import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// One message in the chat view. Yours on the right in the accent colour,
// others on the left. Newsletters and other bulk mail show as a card.
Item {
    id: bubble

    property Theme theme
    property var message
    property bool isGroup: false
    property string html: ""
    signal htmlRequested(int messageId)

    readonly property bool mine: message.is_me
    readonly property bool sending: message.delivery === "sending"
    readonly property bool failed: message.delivery === "failed"
    property bool expanded: false
    property bool showOriginal: false

    implicitHeight: column.implicitHeight + 4
    Accessible.role: Accessible.StaticText
    Accessible.name: (mine ? qsTr("You") : message.sender_name) + ", " + message.time_label + ": " + message.text

    ColumnLayout {
        id: column
        width: parent.width
        spacing: 2

        Text {
            visible: !!bubble.message.subject_change
            text: qsTr("Subject: %1").arg(bubble.message.subject_change || "")
            color: bubble.theme.faint
            font.pointSize: bubble.theme.tiny
            font.italic: true
            horizontalAlignment: Text.AlignHCenter
            Layout.fillWidth: true
            Layout.topMargin: 8
            Layout.bottomMargin: 4
            elide: Text.ElideRight
        }

        Text {
            visible: bubble.message.show_sender
            text: bubble.message.sender_name
            color: bubble.theme.subtext
            font.pointSize: bubble.theme.tiny
            font.bold: true
            Layout.leftMargin: bubble.isGroup ? 44 : 4
            Layout.topMargin: 6
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            layoutDirection: bubble.mine ? Qt.RightToLeft : Qt.LeftToRight

            Item {
                visible: bubble.isGroup && !bubble.mine
                implicitWidth: 32
                implicitHeight: 32
                Layout.alignment: Qt.AlignBottom
                Avatar {
                    visible: bubble.message.show_sender
                    anchors.fill: parent
                    size: 32
                    initials: bubble.message.avatar.initials
                    tint: bubble.message.avatar.color
                }
            }

            Rectangle {
                id: body
                readonly property real maxWidth: column.width * (bubble.message.is_card ? 0.85 : 0.72)
                // Size the bubble to its text's natural width, up to the maximum.
                readonly property real naturalWidth: Math.max(Math.ceil(measure.implicitWidth) + 6,
                    bubble.message.attachments.length > 0 ? 240 : 0,
                    bubble.message.is_card ? maxWidth : 0) + 28
                Layout.preferredWidth: Math.min(maxWidth, Math.max(naturalWidth, 80))
                implicitHeight: bubbleContent.implicitHeight + 18
                radius: bubble.theme.radius
                color: bubble.message.is_card ? bubble.theme.card
                    : (bubble.mine ? bubble.theme.accent : bubble.theme.bubbleOther)
                border.width: bubble.message.is_card ? 1 : 0
                border.color: bubble.theme.border
                opacity: bubble.sending ? 0.75 : 1.0

                readonly property color ink: bubble.mine && !bubble.message.is_card ? bubble.theme.accentText : bubble.theme.text
                readonly property color softInk: bubble.mine && !bubble.message.is_card ? bubble.theme.accentSubtext : bubble.theme.subtext

                ColumnLayout {
                    id: bubbleContent
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.top: parent.top
                    anchors.margins: 14
                    anchors.topMargin: 9
                    spacing: 6

                    Text {
                        visible: bubble.message.is_card
                        text: bubble.message.sender_name
                        color: body.ink
                        font.pointSize: bubble.theme.small
                        font.bold: true
                    }

                    TextEdit {
                        id: textBody
                        text: bubble.message.text
                        color: body.ink
                        readOnly: true
                        selectByMouse: true
                        selectionColor: bubble.mine ? "#1d4fa8" : bubble.theme.selection
                        wrapMode: TextEdit.Wrap
                        textFormat: TextEdit.PlainText
                        font.pointSize: bubble.theme.base
                        Layout.fillWidth: true
                    }

                    Text {
                        id: measure
                        visible: false
                        text: bubble.message.text
                        textFormat: Text.PlainText
                        font: textBody.font
                    }

                    Text {
                        visible: bubble.message.forwarded
                        text: qsTr("Forwarded message")
                        color: body.softInk
                        font.pointSize: bubble.theme.tiny
                        font.italic: true
                    }

                    // Quoted history and signature, behind a "…" toggle.
                    AbstractButton {
                        id: more
                        visible: bubble.message.has_hidden
                        implicitWidth: dots.implicitWidth + 16
                        implicitHeight: dots.implicitHeight + 4
                        Accessible.name: bubble.expanded ? qsTr("Hide quoted text") : qsTr("Show quoted text")
                        onClicked: bubble.expanded = !bubble.expanded
                        background: Rectangle {
                            radius: 6
                            color: bubble.mine && !bubble.message.is_card ? "#3d7ce3" : bubble.theme.border
                            border.width: more.visualFocus ? 1 : 0
                            border.color: body.ink
                        }
                        contentItem: Text {
                            id: dots
                            text: bubble.expanded ? qsTr("Hide") : "•••"
                            color: body.ink
                            font.pointSize: bubble.theme.tiny
                            horizontalAlignment: Text.AlignHCenter
                        }
                    }

                    TextEdit {
                        visible: bubble.expanded
                        text: [bubble.message.signature, bubble.message.quoted].filter(function (s) { return !!s }).join("\n\n")
                        color: body.softInk
                        readOnly: true
                        selectByMouse: true
                        wrapMode: TextEdit.Wrap
                        textFormat: TextEdit.PlainText
                        font.pointSize: bubble.theme.small
                        Layout.fillWidth: true
                    }

                    // Sanitised HTML of the full email (remote images blocked).
                    AbstractButton {
                        visible: bubble.message.has_html && bubble.message.is_card
                        implicitWidth: origText.implicitWidth
                        implicitHeight: origText.implicitHeight
                        Accessible.name: origText.text
                        onClicked: {
                            bubble.showOriginal = !bubble.showOriginal
                            if (bubble.showOriginal)
                                bubble.htmlRequested(bubble.message.message_id)
                        }
                        contentItem: Text {
                            id: origText
                            text: bubble.showOriginal ? qsTr("Hide full email") : qsTr("Show full email")
                            color: bubble.theme.accent
                            font.pointSize: bubble.theme.small
                            font.underline: true
                        }
                    }

                    Text {
                        visible: bubble.showOriginal && bubble.html !== ""
                        text: bubble.html
                        textFormat: Text.RichText
                        wrapMode: Text.Wrap
                        color: body.ink
                        font.pointSize: bubble.theme.small
                        Layout.fillWidth: true
                        onLinkActivated: function (link) { Qt.openUrlExternally(link) }
                    }

                    Flow {
                        visible: bubble.message.attachments.length > 0
                        spacing: 6
                        Layout.fillWidth: true
                        Repeater {
                            model: bubble.message.attachments
                            delegate: Rectangle {
                                required property var modelData
                                radius: 8
                                color: bubble.mine && !bubble.message.is_card ? "#3d7ce3" : bubble.theme.window
                                border.color: bubble.theme.border
                                border.width: bubble.mine ? 0 : 1
                                implicitWidth: chip.implicitWidth + 16
                                implicitHeight: chip.implicitHeight + 10
                                Accessible.name: qsTr("Attachment %1, %2").arg(modelData.filename).arg(modelData.size_label)
                                Text {
                                    id: chip
                                    anchors.centerIn: parent
                                    text: "📎 " + parent.modelData.filename + "  ·  " + parent.modelData.size_label
                                    color: body.ink
                                    font.pointSize: bubble.theme.tiny
                                }
                            }
                        }
                    }
                }
            }

            Item { Layout.fillWidth: true }
        }

        RowLayout {
            spacing: 4
            layoutDirection: bubble.mine ? Qt.RightToLeft : Qt.LeftToRight
            Layout.fillWidth: true
            Layout.leftMargin: bubble.isGroup && !bubble.mine ? 44 : 6
            Layout.rightMargin: 6

            Text {
                text: bubble.sending ? qsTr("Sending…")
                    : (bubble.failed ? qsTr("Not sent, will retry") : bubble.message.time_label)
                color: bubble.failed ? bubble.theme.danger : bubble.theme.faint
                font.pointSize: bubble.theme.tiny
            }
            Repeater {
                model: bubble.message.account_colors
                delegate: Rectangle {
                    required property var modelData
                    width: 6; height: 6; radius: 3
                    color: modelData
                }
            }
            Item { Layout.fillWidth: true }
        }
    }
}
