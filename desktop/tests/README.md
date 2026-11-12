<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# desktop/tests/

```text
screenshots/   reference images, compared by the `screenshots` job
e2e/           scripts run in the boot-test guest (task D-05)
compositor/    tests that need a running KWin (task D-15)
```

`screenshots/` holds one PNG for each picture the `screenshots` job renders,
named `<page>-<variant>-<direction>-<scale>.png`, and `probe-1x.png` for the
toolchain proof. They are made by CI, looked at, and committed; how is in
[desktop/README.md](../README.md), "Reference images". Never edit one by hand.

Unit tests and QML tests live beside the code they test: `kit/tests/`, and
`tests/` in each program.
