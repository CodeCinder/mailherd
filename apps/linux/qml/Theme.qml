import QtQuick

// Colours and sizes for the whole app. Follows the system light/dark setting
// and the system font size.
QtObject {
    id: theme

    property SystemPalette palette: SystemPalette { colorGroup: SystemPalette.Active }
    readonly property bool dark: palette.window.hslLightness < 0.5

    readonly property color window: dark ? "#15171c" : "#ffffff"
    readonly property color sidebar: dark ? "#1b1e24" : "#f3f4f6"
    readonly property color list: dark ? "#181a20" : "#fafafb"
    readonly property color border: dark ? "#2a2e37" : "#e2e4e8"
    readonly property color text: dark ? "#e9eaee" : "#1a1c20"
    readonly property color subtext: dark ? "#9ba1aa" : "#5d636c"
    readonly property color faint: dark ? "#6c727c" : "#8a9099"
    readonly property color accent: "#2f6fdb"
    readonly property color accentText: "#ffffff"
    readonly property color accentSubtext: "#dbe6fb"
    readonly property color bubbleOther: dark ? "#262a33" : "#eceef2"
    readonly property color selection: dark ? "#263250" : "#e2ebfb"
    readonly property color hover: dark ? "#1f232b" : "#f0f2f5"
    readonly property color card: dark ? "#1e2229" : "#ffffff"
    readonly property color danger: "#d93025"
    readonly property color badge: accent

    // Font sizes scale with the user's system font setting.
    readonly property real base: Qt.application.font.pointSize > 0 ? Qt.application.font.pointSize : 10.5
    readonly property real small: base * 0.85
    readonly property real tiny: base * 0.75
    readonly property real title: base * 1.25

    readonly property int radius: 16
    readonly property int gap: 8
}
