<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Session 01: First use

Capture: `session-01.pcapng` (readable form: `session-01.txt`), 120 USB events.
Times are seconds since the capture started, on the same clock as the capture.
Each "did" entry is one run of the gadget's Windows program; "saw" is what it printed.
Entries with a single time are things that happened while the program was running.

- **0.500 - 14.773 s**: did: started the computer with the gadget plugged in (Windows loaded its driver)
  - saw: nothing (no output)
- **14.773 - 15.015 s**: did: asked for the device information
  - saw:

    ```text
    firmware 3.1.4
    uptime 4305
    ```

- **15.717 - 15.729 s**: did: read all sensors
  - saw:

    ```text
    temperature 21.37
    humidity 45.2
    light 312
    ```

- **16.429 - 16.445 s**: did: asked which mode the gadget is in
  - saw:

    ```text
    day
    ```

- **17.145 - 17.158 s**: did: asked for the display brightness
  - saw:

    ```text
    60
    ```

- **17.859 - 18.377 s**: did: asked for the night-light level
  - saw:

    ```text
    10
    ```

- **19.078 - 19.592 s**: did: asked for the temperature alert
  - saw:

    ```text
    off
    ```

- **20.293 - 20.305 s**: did: listed the stored notes (there were none)
  - saw: nothing (no output)
