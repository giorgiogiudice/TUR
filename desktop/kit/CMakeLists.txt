# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# tur-kit: the QML module "Tur.Kit" and its small library, libturkit.

set(kit_qml
    tokens/Theme.qml
    tokens/Motion.qml
    controls/Banner.qml
    controls/BusyIndicator.qml
    controls/Button.qml
    controls/Card.qml
    controls/CheckBox.qml
    controls/CodeLabel.qml
    controls/Disclosure.qml
    controls/FocusRing.qml
    controls/Glyph.qml
    controls/Label.qml
    controls/ListRow.qml
    controls/Page.qml
    controls/ProgressBar.qml
    controls/Separator.qml
    controls/Sheet.qml
    controls/StateLayer.qml
    controls/Switch.qml
    controls/TextField.qml
    message/Message.qml
)

# The sources are sorted into directories; the module is flat, so that every
# type sees every other without an import.
foreach(file IN LISTS kit_qml)
    get_filename_component(name ${file} NAME)
    set_source_files_properties(${file} PROPERTIES QT_RESOURCE_ALIAS ${name})
endforeach()
set_source_files_properties(tokens/Theme.qml tokens/Motion.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)

# NO_CACHEGEN: no ahead-of-time compilation of QML to C++, until the legal
# review has answered whether generated code carries an obligation
# (desktop-experience.md, section 4, "Tools are not linked").
qt_add_qml_module(turkit
    URI Tur.Kit
    VERSION 1.0
    SHARED
    NO_CACHEGEN
    SOURCES
        turkit.h
        turkit.cpp
        tokens/systemappearance.h
        tokens/systemappearance.cpp
    QML_FILES ${kit_qml}
)
target_include_directories(turkit PUBLIC ${CMAKE_CURRENT_SOURCE_DIR} ${CMAKE_CURRENT_SOURCE_DIR}/tokens)
target_link_libraries(turkit PUBLIC Qt6::Core Qt6::Gui Qt6::Qml Qt6::Quick PRIVATE Qt6::QuickControls2)

add_subdirectory(pax)
add_subdirectory(a11y)
add_subdirectory(gallery)
add_subdirectory(tests)
