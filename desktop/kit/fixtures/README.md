<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# desktop/kit/fixtures/

The fixture loader: every program takes `--fixture DIR` and then answers from
JSON files instead of D-Bus, so that it can be built and tested before the
services exist
([desktop-experience.md](../../../docs/architecture/desktop-experience.md),
section 15.2). Empty: it arrives with the first program that talks to a
service (task D-07 or D-10, whichever is first).
