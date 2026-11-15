# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Tur.Kit.Pax: P@x's gold and the few things drawn in it. A module of its
# own, so that a program that is not one of P@x's surfaces cannot use gold by
# accident: it would have to import this, and the boundary check refuses.

set_source_files_properties(PaxTheme.qml PaxCommands.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)

qt_add_qml_module(turkitpax
    URI Tur.Kit.Pax
    VERSION 1.0
    SHARED
    NO_CACHEGEN
    QML_FILES
        PaxTheme.qml
        PaxCommands.qml
        PaxMark.qml
        PaxFlame.qml
        PaxName.qml
        PaxPill.qml
)
target_link_libraries(turkitpax PRIVATE Qt6::Core Qt6::Qml Qt6::Quick)
add_dependencies(turkitpax turkit)
tur_install_modules(turkitpax)
