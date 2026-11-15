<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# tur-pax-ui: the window where a person talks to P@x

**Task:** D-17 of [desktop-experience.md](desktop-experience.md), section 15.5
**Design:** [desktop-experience.md](desktop-experience.md), sections 9.1, 9.2,
9.11, 9.12 and 17.1; [ai-control-layer.md](ai-control-layer.md), sections 7,
10, 21 and 22
**Talks to:** `org.tur.Agent1`, P@x's session service, as built
([pax-service.md](pax-service.md))
**Code:** `desktop/apps/tur-pax-ui/`
**Report:** [desktop-6](../reports/desktop-6.md)

`tur-pax-ui` is P@x's window: the ask bar, the conversation panel in which
each step P@x takes appears as it happens, P@x's record of what it did, and
P@x's setup. It is a standalone Qt Quick program on `Tur.Kit` and
`Tur.Kit.Pax`, a `QGuiApplication`, under the project licence. It holds no
authority of its own: everything it does is a call on `org.tur.Agent1` that
any program of the person could make (pax-service.md, section 3), and the
service decides. It keeps no key and no conversation on disk.

**On the image** since TUR-011 (issue #59, #106 item 1): section 9 says
where.

---

## 1. What is built

| Part | Design | Built |
|---|---|---|
| Ask bar | 9.2: one line, 640 px, opened by P@x's key, closed by Esc or when it loses the keyboard while empty | Yes. Enter sends; the bar becomes the panel when the task starts. Voice is shown as "coming later" with the control there and unavailable |
| Conversation panel | 9.2: 420 px along the right edge; header with the AI and the cost; the person's words, P@x's words and the system's apart; steps with "Details"; Stop and Pause with their keys while a task runs; a field that takes words at any time | Yes, all of it (section 4). "Show me / Do it for me" is not: the service has no guide mode (section 10) |
| Activity | Settings ▸ P@x ▸ Activity (9.11, task D-19) | A view in the panel over `ListActivity`; the settings page is D-19's |
| P@x is off, not ready, not there | 9.1: the small card with "Set up P@x" | Yes, plus the service's own reason and code when P@x is on but cannot take a task, and a plain message when the service is absent |
| Setup | 9.12: seven screens | Five of seven, with local AI added as a way in, generation settings added, and screens 6 and 7 left out because what they set up does not exist (section 5) |
| P@x's keys | 9.1 and 17.1.2: Meta+Space opens the bar; the stop key and P@x's keys visible at all times while P@x is on, from one source | Bound by the desktop file from `commands.json`; shown in the bar and the panel from the same file (section 6) |
| Surface | 9.2: a layer-shell surface, top layer, keyboard on demand | Under Wayland, through LayerShellQt; an ordinary window elsewhere (section 2) |

## 2. Windows

```text
 Main.qml (QtObject)
 ├── client: AgentClient ── org.tur.Agent1, session bus
 ├── conversation: Conversation
 ├── surface: Window "paxSurface"          layer-shell surface under Wayland
 │     ├── AskBar               mode "ask"       640 px, anchored to the bottom edge, 96 px up
 │     └── ConversationPanel    mode "panel"     420 px, anchored right, top to bottom
 │                              mode "activity"  the same, on its Activity page
 └── setupWindow: Window "paxSetup"         an ordinary window, mode "setup"
       └── SetupFlow
```

`mode` is what is shown: nothing, `ask`, `panel`, `activity` or `setup`. The
bar becomes the panel when the conversation wants attention (a task started
or ended, a refusal). The program (`src/surface.cpp`) makes the surface a
layer-shell surface before it is first mapped (`Main.ready` stays false until
then): the top layer, keyboard interactivity "on demand", exclusive zone 0
(it pushes no window aside), on the screen the person is working on; it hides
the surface, changes its anchors and shows it again when it changes from bar
to panel. The setup is an ordinary window because it is a task of its own,
which may be long and is never over other windows by surprise.

**One window per person.** The program owns `org.tur.PaxUi1` on the session
bus (`dbus/org.tur.PaxUi1.xml`, interface version 1): `Show(mode)` and
`Hide()`. A second start (P@x's key pressed again) calls `Show` on the first
and ends. The tray item of P@x (`tur-notifier`, task D-09) and the launcher's
"Ask P@x" result (task D-13) call the same; `data/org.tur.PaxUi1.service`
starts the program with nothing shown (`--background`) when they call and it
is not running. The interface carries no words to P@x: a program that wants
P@x to do something calls `org.tur.Agent1`, under its rules.

**Command line.** `tur-pax-ui [--mode ask|panel|activity|setup]
[--background] [--variant dark|light] [--standalone] [--no-dbus] [--check]
[--bus ADDRESS]`, and the keys' commands, which need no display: `--stop`,
`--show-keys` (section 6).

## 3. org.tur.Agent1

`AgentClient` (`src/agentclient.*`, a QML type) is the program's side of the
interface.

- **Every method**, asynchronously, through `call(method, arguments,
  callback)`. The table of methods and the D-Bus type of each argument is
  the committed interface file's; `tst_paxui_client` reads
  `services/tur-pax/interfaces/org.tur.Agent1.xml` and fails if the two
  differ. A name that is not a method is refused before anything is sent.
- **Replies** that the service sends as JSON text arrive parsed. A refusal
  arrives as its `TUR-AI-*` code (`org.tur.Agent1.Error.TUR_AI_KEY_004` →
  `TUR-AI-KEY-004`) and the service's own sentence, which is what the window
  shows; the window adds no words of its own to a refusal.
- **Signals** (`TaskStarted`, `TextAdded`, `ActionProposed`,
  `ActionCompleted`, `TaskFinished`, `LocalModelProgress`, `StateChanged`, …)
  are passed on with their JSON parsed.
- **Presence.** Present when the name is owned or the bus can start it;
  absent otherwise, with the reason. The window, which a person opened, may
  start the service through D-Bus activation; the stop key never does
  (`autoStart` false, and `--stop` sends its call with the "no auto start"
  flag).
- **Status.** `GetStatus` at start, on `StateChanged`, after every response
  (`TextAdded`, `ActionCompleted`, `TaskFinished`) and after the setup
  changes something; requests that come while one is on its way are folded
  into one more.
- **The key** goes into one `SetKey` message and is not stored, logged or
  repeated. `tst_paxui_client` checks every property of the client after a
  key was set, and the stand-in reports only the key's length.

`Conversation` (`src/conversation.*`, a list model) builds what the panel
shows from the signals, in the order the service sends them:

| Row | From | Notes |
|---|---|---|
| `you` | the request typed here (`pending` until `TaskStarted`, then `started` with the service's time), or a task another program started (`elsewhere`) | |
| `pax` | `TextAdded` `interim` and `answer` | P@x's words, in the first person, under P@x's name in gold |
| `step` | `ActionProposed`, then the same row updated by `ActionCompleted` (matched by task and the step's `index`; the audit identifier differs between the two) | A step still running when the task ends is shown as stopped |
| `system` | a refusal of `StartTask`; `TextAdded` `notice`; a task that ended other than completed; a task a schedule started | The system speaking, in the third person, in a bordered card with TUR's name and the code. A notice that the end of the task repeats (the day's limit) becomes one card, said once |
| `summary` | `TaskFinished` `completed` | Counts of steps done and refused, and the task's cost or tokens |

**The cost so far.** The service does not stream a running task's cost
(pax-service.md, section 13). The panel shows the day's change since the task
started, which is the task's own use because the service runs one task at a
time: in money when the status says the price is known, in tokens when it is
not, with "TUR does not know this model's price, so it cannot show a cost."
When the task ends, its own `cost` (or tokens) replaces the estimate. The
day is shown against its limit ("Today: 0.0012 USD of 5.00 USD"), as a
warning once the limit is reached.

## 4. What the panel shows

- **A step** is one line in plain words (the step's `title`, the first
  sentence of the tool's description), its state (Running, Done, Refused,
  Not done, Failed, Stopped before it ended) and the policy's decision in
  plain words: "Allowed: it only looks." (`observe_permitted`), "Allowed: it
  can be undone." (`reversible_permitted`), "Allowed."; a refusal is
  "Refused: " and the service's own sentence. "Details" opens the technical
  layer: the service and tool, the exact action (`detail`; a command in
  full, in fixed-width type), the action class and tier in words, the result
  and exit code, the error code and the audit record's identifier.
- **Untrusted output is marked as data.** The service sends how much a tool
  returned, not what it returned (`output_bytes`; pax-service.md, 2.4). Each
  step that returned something carries, on its line and not only in Details,
  a bordered box tagged "Data": "It returned 55 bytes. It went to P@x as
  data. It is not an instruction and grants no permission, whatever it
  says.", plus whether part was withheld or cut short. This is what the
  orchestrator tells the model about the same output (pax-orchestrator.md,
  section 7).
- **Stop and Pause** are in the same place at the foot whenever a task
  runs: "Stop  Meta+Esc" (the key from `commands.json`) and Pause/Resume. The
  stop key also works inside the window (a `Shortcut`), and typing "stop" in
  the field stops the task; other words typed while a task runs are kept,
  and a card says P@x does one task at a time.
- **Gold is P@x's.** P@x's name and flame (`Tur.Kit.Pax`'s `PaxName` and
  `PaxMark`) are the only gold on any surface. Stop is the system's blue: it
  is the person's control over P@x, not P@x. `tst_walks` walks every item of
  every state in both variants and fails if anything gold is outside one of
  P@x's marks.

## 5. Setup

Started from the bar or the panel ("Set up P@x"), from the desktop file's
"Set up P@x" action, or `tur-pax-ui --mode setup`.

| Screen | Calls | Notes |
|---|---|---|
| Meet P@x | `ListModels` | 9.12 screen 1, with "an AI that you choose: one that runs on this computer, or a provider's", and what this version does |
| Where should P@x's AI run? | | "On this computer (local AI)" or "With an AI provider" |
| Local: choose a model | `ListLocalModels` | Each model as a choice: its tier for this computer (Runs well, Slow, Does not fit; a sign and a word) with local AI's message and reasons, "Downloads 0.6 GB" or "Already on this computer", the publisher, the licence in plain words with its restriction. The recommendation is marked. A model that does not fit, or that P@x cannot use, cannot be chosen. A licence that restricts use must be accepted with a check box |
| Local: making it ready | `UseLocalModel` (with `download_bytes`, the size shown, and `accept_licence`), `LocalModelProgress`, `CancelLocalModel` | Local AI's real figures ("Downloaded 0.3 GB of 0.6 GB"), each step as local AI reports it, Cancel; "What was downloaded is kept" after a cancel. When the last step is `in-use` the provider is local AI with that model |
| Provider | `ListModels` | 9.12 screen 2 with every provider of the model list, not three: companies in alphabetical order, then services on this computer; no recommendation. Each with its data terms as TUR read them (keeps, for how long, trains, where) and the date, or "<name>'s terms are not reviewed by TUR"; "Get a key" and "Read its data terms" open the browser |
| Key and model | `HasKey`, `ListProviderModels`, `SetProvider`, `SetKey` | 9.12 screen 4. The key field is a password field; it is emptied the moment Continue is pressed and the key goes to the service in one call. A stored key is said to be there and never shown. Models from TUR's list ("Not tested with TUR", "Price not known to TUR", no pictures) or a name typed by hand. "Check the key" is not built: it would send a request the provider bills (pax-service.md, section 13) |
| How the model answers | `GetGenerationSettings`, `SetGenerationSettings` | Only what the model accepts: the reasoning effort as a choice among the model's values or P@x's default; temperature, top-p, top-k, thinking budget, output limit and context as fields with their range and default. Checked here first ("Temperature can be from 0 to 2."), then by the service, whose refusal is shown in its own words ("This model takes temperature, top-p and top-k only when its reasoning effort is none …"). Stop sequences are left to `tur-pax models settings` |
| Spending limits | `SetLimits` | 9.12 screen 5: per task and per day, in money when the price is known and in tokens otherwise, with the sentences of 9.11 |
| What will be sent | `GetConsentText`, `Enable` | 9.12 screen 3, moved here so that nothing is switched on before the last screen. The service's text word for word (with what TUR read of the provider's terms in it); "Switch P@x on" stays unavailable until "I understand what will be sent to <provider>" is ticked, and calls `Enable` with the version of the text that was shown, which the service records with the provider and the time |
| P@x is ready | | "Press Meta+Space and type. Press Meta+Esc to stop P@x at any time." and P@x's keys |

**Left out**, as 9.12 says for the trusted screen: screen 6 (seeing and using
the screen: the service cannot, in this version) and screen 7 (the trusted
screen: task D-16). The ready screen says that P@x looks and makes changes
that can be undone, and nothing more.

**Order.** The design asks for consent before the key. Here the consent
screen comes last because "nothing is enabled until the last" (9.12) and
`Enable` records the consent for the provider in use, which is chosen
(`SetProvider`) on the key screen. The text is shown before anything is
switched on either way; the key is stored before it.

## 6. P@x's keys

`desktop/kit/pax/commands.json` is the one source (17.1, answer 2).

| Command | Keys | Bound by `data/org.tur.PaxUi.desktop` | Runs |
|---|---|---|---|
| `ask` | Meta+Space | the entry's `X-KDE-Shortcuts` | `tur-pax-ui --mode ask`: the bar, or the open window shows it |
| `stop` | Meta+Esc | the action `stop` | `tur-pax-ui --stop`: `CancelTask("")` with "no auto start": stops the running task; if P@x is not running it says so and starts nothing |
| `keys` | Meta+/ | the action `keys` | `tur-pax-ui --show-keys`: toggles the list on P@x's indicator (`org.tur.PaxOverlay1.ShowCommands`) |
| `talk` | Hold Meta+Space | nothing | Voice is not built |

Each group that binds a key names its command (`X-TUR-Pax-Command`);
`gen_commands.py --check` (the `boundary` job) now reads every group of a
desktop file, not only the first, and fails if a key differs from
`commands.json` or a command marked bound has no binding. The bar and the
panel show the bound commands from `PaxCommands`, generated from the same
file. Whether Meta+Space and Meta+Esc are free in Plasma 6.8's defaults, and
whether KDE's shortcut service binds a desktop action's `X-KDE-Shortcuts`
from a file in `/usr/share/applications`, are **not verified** (`verified`
stays false); the image test can check both once the program is installed.

## 7. Tests

All in CI, in the desktop workflow.

**The fixture.** `tests/fixture/agent.cpp` (`tur-pax-ui-fixture-agent`) is a
stand-in for `org.tur.Agent1` that owns the name on a private bus (a
`dbus-daemon` the test starts) and answers from `fixtures/agent1/`: recorded
replies keyed by method and arguments, recorded statuses, and every signal
of recorded tasks and local downloads. It decides nothing: a setting the
recording does not refuse is accepted and noted; `Enable` and `Disable` and a
local model becoming ready move to another recorded status; a recorded task
plays signal after signal, and one the service ended because it was
cancelled holds before its end until `CancelTask`, as a download that was
cancelled holds until `CancelLocalModel`. The tests steer it over
`org.tur.Agent1Fixture` (which recording, where to hold, what was called,
whether a key was given).

**Nobody writes the fixture.** The job `pax-ui-end-to-end` records it on
every run (`desktop/tests/pax-ui/e2e.py record`) from `tur-pax serve`
running exactly as the `pax` workflow runs it (`tests/pax/service_e2e.py`'s
private bus, gnome-keyring, bubblewrap, the test service's tools, the
stand-in for local AI, the scripted provider), and fails when the committed
files differ from it in anything but times and identifiers.
`fixtures/update.py RUN_ID` takes a run's recording.

| Test | What it proves |
|---|---|
| `tst_paxui_conversation` | Fed the recorded signals directly: a task streams in order (each step once, proposed then ended); a refused step carries the service's reason and code; a task stopped mid-step marks the step stopped and says so with `TUR-AI-TASK-003`; the day's limit is said once, by the system, with `TUR-AI-SPEND-001`; a refusal before the start is a card; tasks asked elsewhere or by a schedule are marked |
| `tst_paxui_client` | The client's table equals the committed interface file; presence; parsed replies; refusals as codes and sentences; signals drive the conversation; a key reaches the service and stays nowhere in the client; `--stop` stops a running task; `--stop` never starts the service although the bus could, while the window may; an absent service |
| `tst_conversation.qml` | Asking streams steps and the answer; steps arrive one signal at a time; a refused step says why; the data mark; Stop by button, by Meta+Esc and by typing "stop"; the spending limit card; cost in money when the price is known; absent, off, not ready; the keys from `commands.json`; Esc closes the bar; voice is later |
| `tst_setup.qml` | Consent gates switching on and `Enable` gets the version shown; reviewed terms in DeepSeek's consent; local models with tiers, sizes, licences, the recommendation; a restricting licence must be accepted; download progress with real figures, then ready; cancel; providers' terms; the key goes to the service and the field forgets it; a refused key; only accepted settings; a setting out of range caught before sending; the service's refusal shown in its words; limits in tokens when the price is unknown; "Not now" switches nothing on |
| `tst_activity.qml` | The recorded activity, newest first; a task opens to its decisions in words |
| `tst_walks.qml` | For every state of `PaxHost.states` (29): contrast in both variants; gold only inside P@x's marks, both variants; keyboard and accessible names left-to-right and right-to-left; no clipped text at 200 %, right-to-left long, and both |
| `pax-ui-starts-*` | The program loads each view with no QML warning, with no session bus (the service absent) |
| `pax-ui-desktop-file`, `gen_commands.py --check` | The desktop file is valid and binds what `commands.json` says |
| Pictures | `tests/shots/render_paxui.qml`: every state, dark and light, compared with `desktop/tests/screenshots/paxui-*` |
| End to end (`e2e.py ui`) | The real `tur-pax serve`, the window's QML test runner in the builder container over the bus's socket in the checkout (`tests/e2e/e2e_ask_stop.qml`): a request, its two steps seen as they came and done, the answer; a second request whose sandboxed command is running, stopped by the Stop button within two seconds. On the runner: the service's own signals (one task completed with its two steps, one cancelled with its code), the command seen running and killed, three requests to the provider |

## 8. Licence boundary

`Tur.PaxUi` and `libturpaxui` are the project's. The program links Qt Core,
Gui, Qml, Quick and D-Bus (LGPL-3.0-only) and, by the program and not by its
module, LayerShellQt (every source file of `src/` "LGPL-2.1-only OR
LGPL-3.0-only OR LicenseRef-KDE-Accepted-LGPL", read file by file at Plasma
6.8.2 for the overlay, tur-pax-overlay.md section 8). `desktop/ALLOWED_IMPORTS`
says so; `Tur.Kit.Pax` (gold) is allowed for `desktop/apps/tur-pax-ui/**`.
The `boundary` job and `boundary.py --linked` check sources and binaries.

## 9. Installation

The install rules are in `desktop/apps/tur-pax-ui/CMakeLists.txt`; the image
build runs them (image/README.md, "The desktop programs"; report TUR-011).

| From | To |
|---|---|
| the built `tur-pax-ui` | `/usr/bin/tur-pax-ui` |
| `libturpaxui.so`, with `libturkit.so` and `libturkitpax.so` | `/usr/lib64/tur/`, the programs' run path. The QML is inside the libraries; no QML import path is needed |
| `data/org.tur.PaxUi.desktop` | `/usr/share/applications/org.tur.PaxUi.desktop` (launcher entry and P@x's three keys) |
| `data/org.tur.PaxUi1.service` | `/usr/share/dbus-1/services/org.tur.PaxUi1.service` |
| `dbus/org.tur.PaxUi1.xml` | `/usr/share/dbus-1/interfaces/` |
| `data/icons/org.tur.Pax.svg`: the kit's flame (`PaxFlame.qml`'s paths) in gold on the dark tile of TUR's icon. A first draft for the owner | `/usr/share/icons/hicolor/scalable/apps/org.tur.Pax.svg`, the icon the desktop file names |
| runtime | LayerShellQt, Qt 6 D-Bus, Quick and Quick Controls 2 are in the base image; Inter and JetBrains Mono are added by the image (`image/desktop/fonts.lock`) |
| `tur-pax` and its units (pax-service.md, section 6) | in the image since TUR-010 |

## 10. What the service lacks

Requirements on `tur-pax` found here; none was changed (`services/**` is not
this task's):

1. **A running task's cost** is not streamed; the panel shows the day's
   change since the task started (section 3). A `cost` and `usage` in each
   `ActionCompleted`, or a `TaskProgress` signal, would make it exact.
2. **What a tool returned** is not on the interface (only `output_bytes`),
   so the panel can mark the output as data but cannot show it. The design's
   "what it read" needs the text, withheld parts removed, in the step.
3. **"Check the key"** (9.12) is not built in the service.
4. **"Allow another 1.00 USD for this task"** (9.11) is not built; it is to
   be confirmed on the trusted screen.
5. **Guide mode** ("Show me / Do it for me", 9.6) and **pause by private
   mode** have nothing to drive in the service.
6. **The request's words in the activity**: `ListActivity` carries no
   request text (the trail keeps no words), so a task in the activity view is
   its time, outcome and actions. `GetTask` has the words for the last 20
   tasks; the view does not join them.
7. **A refusal notice's code**: `TextAdded` `notice` carries no code; the
   code comes with `TaskFinished`.

## 11. Decisions the owner may want to reverse

1. **Every provider in setup, not three.** The design lists Anthropic,
   Google and OpenAI; the model list now has eleven presets including
   DeepSeek and services on this computer, so all are listed, companies
   alphabetically, with each one's data terms.
2. **Local AI as a way in of its own**, before the provider list, since it
   needs no key and sends nothing to a company.
3. **Consent last** in the flow, so that nothing is switched on before the
   last screen (section 5).
4. **Stop bound by the desktop file now** (Meta+Esc runs `tur-pax-ui
   --stop`), before the physical input witness exists; `commands.json` marks
   `stop`, `ask` and `keys` as bound by a shipped file.
5. **`org.tur.PaxUi1`**, a new, small interface (Show, Hide) for the tray
   item and the launcher; frozen as version 1 in `dbus/`; no ADR.
6. **Stop is blue**, not gold: it is the person's control over P@x.
7. **The setup is an ordinary window**, not the layer-shell surface.
8. **The window may start the service** through D-Bus activation when a
   person opens it while P@x is off (to read its status); nothing else of
   tur-pax-ui does.

## 12. Known gaps

- **Not run under KWin or on the image.** The layer-shell surface (anchors,
  keyboard on demand, size of the panel) is not verified on a compositor;
  the compositor job does not run tur-pax-ui. Under the offscreen platform
  it is an ordinary window.
- **The keys are not verified** to be free in Plasma 6.8, nor that KDE's
  shortcut service binds a desktop action's key (section 6).
- **No collapse to a pill** while P@x works on the screen (9.2): P@x does
  not work on the screen in this version.
- **No real model, no real provider**: every answer in every test is the
  scripted provider's; the pictures show the scripted model's words.
- **Voice** is not built.
- **Announcements** to a screen reader when a step arrives (X4) are not
  made: the kit has none yet.
- **No collapse of long conversations**; the panel keeps every row of the
  session in memory and writes none to disk.
