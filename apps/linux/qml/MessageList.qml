import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Middle pane: threads or people, with a Threads/People switch on top.
Rectangle {
    id: list

    property Theme theme
    property string title: ""
    property string mode: "threads"
    property var model: []
    property string selectedId: ""
    signal selected(string id)
    signal modeRequested(string mode)

    color: theme.list

    // Threads and People rows have different fields; give the delegate one shape.
    // Rows are told apart by their fields, since the mode can change a moment
    // before the new rows arrive.
    readonly property var rows: model.map(function (r) {
        if (r.key !== undefined) {
            return {
                id: "people:" + r.key, title: r.name, subject: r.thread_count > 1
                    ? qsTr("%1 conversations").arg(r.thread_count) : "",
                snippet: r.snippet, date: r.date_label, unread: r.unread, pinned: r.pinned,
                attachments: false, colors: [], avatar: r.avatar, count: 0
            }
        }
        return {
            id: r.thread_id, title: r.participants, subject: r.subject, snippet: r.snippet,
            date: r.date_label, unread: r.unread, pinned: r.pinned, attachments: r.has_attachments,
            colors: r.account_colors, avatar: r.avatar, count: r.message_count
        }
    })

    function indexOfSelected() {
        for (var i = 0; i < rows.length; i++)
            if (rows[i].id === selectedId)
                return i
        return -1
    }

    function move(step) {
        if (rows.length === 0)
            return
        var i = indexOfSelected()
        i = i < 0 ? 0 : Math.max(0, Math.min(rows.length - 1, i + step))
        selected(rows[i].id)
        view.positionViewAtIndex(i, ListView.Contain)
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.margins: 14
            Layout.bottomMargin: 8
            spacing: 8

            Text {
                text: list.title
                color: list.theme.text
                font.pointSize: list.theme.title
                font.bold: true
                elide: Text.ElideRight
                Layout.fillWidth: true
            }

            // Threads | People switch.
            Rectangle {
                radius: 8
                color: list.theme.border
                implicitWidth: switchRow.implicitWidth + 4
                implicitHeight: switchRow.implicitHeight + 4
                Row {
                    id: switchRow
                    anchors.centerIn: parent
                    Repeater {
                        model: [{ key: "threads", label: qsTr("Threads") }, { key: "people", label: qsTr("People") }]
                        delegate: AbstractButton {
                            id: seg
                            required property var modelData
                            readonly property bool active: list.mode === modelData.key
                            implicitWidth: segText.implicitWidth + 20
                            implicitHeight: segText.implicitHeight + 10
                            Accessible.name: modelData.label
                            Accessible.checkable: true
                            Accessible.checked: active
                            onClicked: list.modeRequested(modelData.key)
                            background: Rectangle {
                                radius: 6
                                color: seg.active ? list.theme.window : "transparent"
                            }
                            contentItem: Text {
                                id: segText
                                text: seg.modelData.label
                                color: seg.active ? list.theme.text : list.theme.subtext
                                font.pointSize: list.theme.small
                                font.bold: seg.active
                                horizontalAlignment: Text.AlignHCenter
                                verticalAlignment: Text.AlignVCenter
                            }
                        }
                    }
                }
            }
        }

        ListView {
            id: view
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: list.rows
            currentIndex: list.indexOfSelected()
            keyNavigationEnabled: false
            ScrollBar.vertical: ScrollBar {}
            Accessible.role: Accessible.List
            Accessible.name: list.title

            delegate: ItemDelegate {
                id: row
                required property var modelData
                readonly property bool isSelected: modelData.id === list.selectedId
                width: ListView.view.width
                implicitHeight: content.implicitHeight + 20
                Accessible.name: (modelData.unread ? qsTr("Unread. ") : "") + modelData.title
                    + ". " + modelData.subject + ". " + modelData.snippet + ". " + modelData.date
                onClicked: list.selected(modelData.id)

                background: Rectangle {
                    color: row.isSelected ? list.theme.selection
                        : (row.hovered ? list.theme.hover : "transparent")
                    Rectangle {
                        anchors.bottom: parent.bottom
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.leftMargin: 64
                        height: 1
                        color: list.theme.border
                    }
                }

                contentItem: RowLayout {
                    id: content
                    spacing: 12

                    Item {
                        Layout.alignment: Qt.AlignTop
                        implicitWidth: 40
                        implicitHeight: 40
                        Avatar {
                            anchors.fill: parent
                            size: 40
                            initials: row.modelData.avatar.initials
                            tint: row.modelData.avatar.color
                        }
                        Rectangle {
                            visible: row.modelData.unread
                            width: 8; height: 8; radius: 4
                            color: list.theme.accent
                            anchors.right: parent.left
                            anchors.rightMargin: 4
                            anchors.verticalCenter: parent.verticalCenter
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 6
                            Text {
                                text: row.modelData.title
                                color: list.theme.text
                                font.pointSize: list.theme.base
                                font.bold: row.modelData.unread
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }
                            Text {
                                visible: row.modelData.count > 1
                                text: row.modelData.count
                                color: list.theme.faint
                                font.pointSize: list.theme.tiny
                            }
                            Text {
                                text: row.modelData.date
                                color: row.modelData.unread ? list.theme.accent : list.theme.faint
                                font.pointSize: list.theme.tiny
                                font.bold: row.modelData.unread
                            }
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            visible: row.modelData.subject !== ""
                            spacing: 6
                            Text {
                                text: row.modelData.subject
                                color: list.theme.text
                                font.pointSize: list.theme.small
                                font.bold: row.modelData.unread
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }
                            Text {
                                visible: row.modelData.pinned
                                text: "★"
                                color: "#e6a700"
                                font.pointSize: list.theme.small
                                Accessible.name: qsTr("Pinned")
                            }
                            Text {
                                visible: row.modelData.attachments
                                text: "📎"
                                font.pointSize: list.theme.tiny
                                Accessible.name: qsTr("Has attachments")
                            }
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 6
                            Text {
                                text: row.modelData.snippet
                                color: list.theme.subtext
                                font.pointSize: list.theme.small
                                wrapMode: Text.WordWrap
                                maximumLineCount: 2
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }
                            Row {
                                spacing: 3
                                Layout.alignment: Qt.AlignBottom
                                Repeater {
                                    model: row.modelData.colors
                                    delegate: Rectangle {
                                        required property var modelData
                                        width: 7; height: 7; radius: 3.5
                                        color: modelData
                                    }
                                }
                            }
                        }
                    }
                }
            }

            Text {
                anchors.centerIn: parent
                visible: list.rows.length === 0
                text: qsTr("Nothing here")
                color: list.theme.faint
                font.pointSize: list.theme.base
            }
        }
    }
}
