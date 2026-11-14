<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Session 09: Evening

Capture: `session-09.pcapng` (readable form: `session-09.txt`), 108 USB events.
Times are seconds since the capture started, on the same clock as the capture.
Each "did" entry is one run of the gadget's Windows program; "saw" is what it printed.
Entries with a single time are things that happened while the program was running.

- **0.500 - 0.512 s**: did: asked for the device information
  - saw:

    ```text
    firmware 3.1.4
    uptime 4370
    ```

- **1.213 - 1.478 s**: did: switched to night mode
  - saw:

    ```text
    mode night
    ```

- **2.178 - 2.697 s**: did: set the (daytime) display brightness to 20
  - saw:

    ```text
    brightness 20
    ```

- **3.398 - 3.414 s**: did: set the night-light level to 1
  - saw:

    ```text
    night-level 1
    ```

- **4.116 - 4.135 s**: did: read all sensors
  - saw:

    ```text
    temperature 19.50
    humidity 48.0
    light 2
    ```

- **4.836 - 10.867 s**: did: watched the gadget for 6 seconds
  - saw:

    ```text
    button 1 0.84
    button 1 0.12
    ```

- **6.387 s**: pressed button 1 (about 0.8 s)
- **7.887 s**: tapped button 1 very briefly
- **11.568 - 11.579 s**: did: asked which mode the gadget is in
  - saw:

    ```text
    night
    ```

- **12.280 - 12.547 s**: did: switched back to day mode
  - saw:

    ```text
    mode day
    ```

