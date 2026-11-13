# pax-7 — P@x through the accessibility tree: the tur-pax-a11y bridge, measured on Qt, GTK, Firefox and Wine

```text
TUR DEVELOPMENT REPORT

Task ID:          pax-7
Subsystem:        AI control layer: tier 2 (the accessibility tree); the
                  policy engine's accessibility actions; P@x's task loop;
                  CI under KWin
Build ID:         not applicable: this task builds no image of its own
Source revision:  pull request 93 (feat/pax-accessibility); the commits and
                  runs are under "Runs"

SUMMARY
- What changed: P@x can read and operate other applications' windows
  through AT-SPI. A new program, tur-pax-a11y, is the only process that
  connects to the accessibility bus; the task loop starts it as a child on
  a pipe and offers a model eleven typed tools (tur-pax run --interface).
  Each call's target is read from the bus, decided by the policy engine
  with new accessibility operations and roles, recorded, read and decided
  again by the bridge, shown with P@x's pointer, and only then done.
  Nothing above class 1 runs: Send, Delete and the like are refused with a
  plain message.
- Why: the owner's founding requirement that the AI can "see and fully
  control all the apps"; addendum A, section 4, tier 2.
- What it does not show: see "Not proven".
```

RESULTS_PLACEHOLDER
