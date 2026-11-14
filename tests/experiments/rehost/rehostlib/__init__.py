# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The re-hosting experiment (docs/reports/exp-rehost-1.md).

A prototype, not a TUR service: it detects the portable runtime inside a
Windows or macOS package without executing it, lays the payload under the
Linux build of the same runtime, and says how to start it. It runs in CI
only (.github/workflows/exp-rehost.yml).
"""
