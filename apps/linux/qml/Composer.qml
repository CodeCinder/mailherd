import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The chat box under a conversation. Enter sends, Shift+Enter adds a line.
Rectangle {
    id: composer

    property Theme theme
    property string replyLabel: ""
    property alias text: input.text
    readonly property bool editing: input.activeFocus
    signal send(string text)

    function focusInput() { input.forceActiveFocus() }
    function clearFocus() { input.focus = false }

    color: theme.window
    implicitHeight: column.implicitHeight + 20

    Rectangle {
        anchors.top: parent.top
        width: parent.width
        height: 1
        color: composer.theme.border
    }

    ColumnLayout {
        id: column
        anchors.fill: parent
        anchors.margins: 10
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        spacing: 6

        Text {
            text: composer.replyLabel
            visible: text !== ""
            color: composer.theme.faint
            font.pointSize: composer.theme.tiny
            elide: Text.ElideRight
            Layout.fillWidth: true
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 8

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: Math.min(160, input.implicitHeight + 16)
                radius: 18
                color: composer.theme.bubbleOther
                border.width: input.activeFocus ? 2 : 0
                border.color: composer.theme.accent

                ScrollView {
                    anchors.fill: parent
                    anchors.leftMargin: 10
                    anchors.rightMargin: 10

                    TextArea {
                        id: input
                        placeholderText: qsTr("Message")
                        placeholderTextColor: composer.theme.faint
                        color: composer.theme.text
                        wrapMode: TextArea.Wrap
                        font.pointSize: composer.theme.base
                        background: null
                        Accessible.name: qsTr("Reply")
                        Accessible.description: composer.replyLabel

                        Keys.onPressed: function (event) {
                            if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter)
                                    && !(event.modifiers & Qt.ShiftModifier)) {
                                event.accepted = true
                                composer.trySend()
                            } else if (event.key === Qt.Key_Escape) {
                                event.accepted = true
                                input.focus = false
                            }
                        }
                    }
                }
            }

            RoundButton {
                id: sendButton
                enabled: input.text.trim().length > 0
                implicitWidth: 40
                implicitHeight: 40
                Accessible.name: qsTr("Send")
                onClicked: composer.trySend()
                background: Rectangle {
                    radius: 20
                    color: sendButton.enabled ? composer.theme.accent : composer.theme.border
                }
                contentItem: Text {
                    text: "➤"
                    color: "#ffffff"
                    font.pointSize: composer.theme.base
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
            }
        }
    }

    function trySend() {
        var t = input.text.trim()
        if (t.length === 0)
            return
        composer.send(t)
    }
}
