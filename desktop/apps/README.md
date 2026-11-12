<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# desktop/apps/

One directory for each of TUR's desktop programs
([desktop-experience.md](../../docs/architecture/desktop-experience.md),
sections 14.1 and 15.1). Each arrives with the task that builds it. So far:
`tur-centre/`, part 1 ([tur-centre.md](../../docs/architecture/tur-centre.md)).

| Directory | Program | Task |
|---|---|---|
| `setup/` | `tur-setup`, first-run setup | D-06 |
| `welcome/` | `tur-welcome` | D-14 |
| `tur-centre/` | `tur-centre`, the application centre | D-10 (built), D-11, D-12 |
| `settings/` | `tur-settings` | D-07 |
| `diagnostics/` | `tur-diagnostics` | D-08 |
| `pax-ui/` | `tur-pax-ui` | D-17 |
| `pax-overlay/` | `tur-pax-overlay` | D-15 |
| `pax-approve/` | `tur-pax-approve` | D-18 |
| `confirm/` | `tur-confirm`, the trusted screen | D-16 |

Each has the same shape:

```text
CMakeLists.txt   src/ (C++)   qml/   fixtures/   tests/   i18n/
data/ (desktop file, icons, registry file, tool manifest)
```

A program is a `QGuiApplication` with one QML module, built on `Tur.Kit`
(see [desktop-kit.md](../../docs/architecture/desktop-kit.md)), and is added
to `desktop/CMakeLists.txt` with `add_subdirectory`.
