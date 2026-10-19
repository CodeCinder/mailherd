use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(QmlModule::new("LanternPost").qml_files([
        "qml/Main.qml",
        "qml/Theme.qml",
        "qml/Avatar.qml",
        "qml/Sidebar.qml",
        "qml/MessageList.qml",
        "qml/Conversation.qml",
        "qml/Bubble.qml",
        "qml/Composer.qml",
        "qml/UndoBar.qml",
    ]))
    .qt_module("Quick")
    .qt_module("QuickControls2")
    .files(["src/bridge.rs"])
    .build();
}
