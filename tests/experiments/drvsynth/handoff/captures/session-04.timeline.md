<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Session 04: Sensors through the day

Capture: `session-04.pcapng` (readable form: `session-04.txt`), 100 USB events.
Times are seconds since the capture started, on the same clock as the capture.
Each "did" entry is one run of the gadget's Windows program; "saw" is what it printed.
Entries with a single time are things that happened while the program was running.

- **0.501 - 0.513 s**: did: read all sensors (early morning, lights off)
  - saw:

    ```text
    temperature 18.05
    humidity 38.1
    light 0
    ```

- **1.214 - 1.232 s**: did: read the temperature (office, afternoon)
  - saw:

    ```text
    23.90
    ```

- **1.933 - 1.949 s**: did: read the humidity
  - saw:

    ```text
    52.7
    ```

- **2.650 - 2.668 s**: did: read the light level
  - saw:

    ```text
    815
    ```

- **3.370 - 3.387 s**: did: read all sensors (gadget out on the balcony in winter)
  - saw:

    ```text
    temperature -1.25
    humidity 81.0
    light 15
    ```

- **4.088 - 4.101 s**: did: read all sensors (gadget in direct sunlight)
  - saw:

    ```text
    temperature 30.42
    humidity 29.9
    light 70210
    ```

- **4.802 - 4.815 s**: did: read all sensors (by the window, cloudy)
  - saw:

    ```text
    temperature 12.00
    humidity 60.0
    light 65535
    ```

- **5.516 - 5.530 s**: did: read all sensors (bathroom after a shower, under a lamp)
  - saw:

    ```text
    temperature 0.07
    humidity 99.9
    light 131072
    ```

- **6.231 - 6.246 s**: did: read the temperature (gadget in the freezer)
  - saw:

    ```text
    -12.34
    ```

- **6.946 - 6.962 s**: did: read the light level
  - saw:

    ```text
    1
    ```

