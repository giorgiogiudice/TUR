<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Session 07: Alert and buttons

Capture: `session-07.pcapng` (readable form: `session-07.txt`), 118 USB events.
Times are seconds since the capture started, on the same clock as the capture.
Each "did" entry is one run of the gadget's Windows program; "saw" is what it printed.
Entries with a single time are things that happened while the program was running.

- **0.501 - 1.018 s**: did: set the temperature alert to 26.5 degrees
  - saw:

    ```text
    alert 26.5
    ```

- **1.719 - 2.238 s**: did: asked for the temperature alert
  - saw:

    ```text
    26.5
    ```

- **2.939 - 12.971 s**: did: watched the gadget for 10 seconds
  - saw:

    ```text
    button 1 0.30
    alert 27.12
    button 2 1.62
    ```

- **4.490 s**: briefly pressed button 1 (about 0.3 s)
- **5.990 s**: the room warmed up a little
- **7.490 s**: the room warmed up past the alert temperature
- **8.990 s**: held button 2 down (about 1.6 s)
- **13.672 - 14.193 s**: did: switched the temperature alert off
  - saw:

    ```text
    alert off
    ```

- **14.894 - 15.411 s**: did: asked for the temperature alert
  - saw:

    ```text
    off
    ```

