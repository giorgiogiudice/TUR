# pax-1 — The P@x orchestrator: task loop, policy gating, read-only sandbox, text interface

```text
TUR DEVELOPMENT REPORT

Task ID:          pax-1
Subsystem:        AI control layer: orchestrator (services/tur-pax)
Build ID:         not applicable: this task builds no image
Source revision:  2648699d9abc (feat/pax-orchestrator, pull request 37),
                  which is the commit the measurements below come from. The
                  commit that adds this report follows it and changes no code.

SUMMARY
- What changed: the parts of P@x are connected. services/tur-pax gains the
  orchestrator: a task loop that sends a typed request to a model through
  the provider layer, builds each proposed action from the reviewed tool
  manifest, has the policy engine decide it, writes the audit record, and
  then performs, refuses or reports that an approval is not available; a
  tool registry with data-driven adapters that perform a tool by running a
  TUR command-line program; a read-only bubblewrap sandbox in which every
  tool program and every command runs; an output filter that withholds
  credential-like text before it reaches the provider; an audit trail of
  the task; and a text interface, "tur-pax run" and "tur-pax chat".
  Manifests and adapters for the read-only tools that exist. A new CI
  workflow that runs all of it with real bubblewrap and real tool programs
  against a scripted provider.
- Why it changed: the policy engine, the provider adapters and the memory
  store were separate libraries with nothing between them (addendum A,
  section 3; milestones A4 and A5 in part).
- What it does not show: NO REAL MODEL WAS CALLED, here or anywhere in the
  development of this change. The provider in every test is a script. No
  API key exists in CI. Nothing here says how well a real model performs a
  task or how often a real model is misled.

FILES
- Added: services/tur-pax/src/{orchestrator,registry,adapter,sandbox,exec,
  redact,trail,faults,textui}.rs,
  services/tur-pax/tests/{orchestrator,injection,run_cli,tools}.rs,
  services/tur-pax/tests/common/mod.rs,
  services/tur-pax/tests/data/test-service.adapters.json,
  services/tur-pax/tools/manifests/{catalogue,diagnostics,runtime}.json,
  services/tur-pax/tools/adapters/{catalogue,device,diagnostics,runtime,
  storage}.json, schemas/pax-tool-adapter.schema.json,
  tests/pax/{e2e.py,fake_provider.py,probe_sockets.py,sandbox-preflight.sh,
  tur-pax-test-tool}, tests/pax/scenarios/*.json (10),
  tests/pax/{manifests,adapters}/fixture.json, .github/workflows/pax.yml,
  docs/architecture/pax-orchestrator.md, docs/reports/pax-1.md
- Modified: services/tur-pax/Cargo.toml (two path dependencies, the
  description), services/tur-pax/Cargo.lock, services/tur-pax/src/lib.rs
  (modules and re-exports), services/tur-pax/src/cli.rs (two subcommands)
- Removed: none

BUILD
- Command: cargo clippy --all-targets --all-features -- -D warnings; cargo
  test --all-targets --all-features; cargo test --doc (workflow "rust");
  cargo build --locked for tur-pax and five tool components (workflow "pax")
- Result: see RUNS below
- Warnings: none from the compiler or clippy

TESTS
- Unit tests: 163 in services/tur-pax (113 before this task; the 113 are
  unchanged and pass). The 50 new ones cover the sandbox plan and the grant
  rules, the adapters, the registry, the output filter, the trail and the
  pieces of the loop. Pass.
- Integration tests: 45 new, in four files, against a scripted provider
  reached through the real Anthropic adapter over a local connection:
  the loop (24), the injection suite (7), the built program (9), the
  shipped tool set (5). The 87 integration tests of the policy engine are
  unchanged and pass.
- End to end: workflow "pax", 197 checks, on a hosted Ubuntu 24.04
  runner with bubblewrap 0.9.0. Pass. Detail below.
- Against a real provider: NOT RUN. No key exists.
- Through the OpenAI or the Google adapter: NOT RUN. The scripted provider
  speaks the Anthropic wire format only.
- Boot tests: NOT RUN. Nothing here can affect boot; the AI layer is off by
  default and nothing depends on it.
- On the TUR image: NOT RUN. The runner is Ubuntu.
- Hardware tests: NOT RUN.
- Performance tests: NOT RUN.
- Resource exhaustion inside the sandbox (fork bomb, memory hog): NOT RUN.
  The limits are read back from inside the sandbox; nothing tried to exceed
  them.

DIAGNOSTICS
- Log paths: GitHub Actions runs below; artifact "pax-evidence" (versions,
  sandbox preflight, results.json, summary.md, and for each scenario the
  script, the transcript, the audit log, the error output and every request
  the scripted provider received)
- Diagnostic bundle path: none
- Relevant error IDs: 20 new codes: TUR-AI-REGISTRY-001 and -002,
  TUR-AI-ADAPTER-001 to -005, TUR-AI-SANDBOX-001 to -004, TUR-AI-TASK-001
  to -008, TUR-AI-OUTPUT-001

RECOVERY
- Rollback procedure: revert the merge. No other component and no existing
  workflow is changed. The program writes one file of its own, the audit
  log ($XDG_STATE_HOME/tur/pax/audit.jsonl), which can be deleted.
- Recovery tested: not applicable.

KNOWN LIMITATIONS
- See docs/architecture/pax-orchestrator.md, sections 9 (known gaps), 10
  (not built) and 13 (findings on the design). The most important: no real
  model; nothing above class 1 can run, by construction; the audit trail is
  a file of the user's, not the journal; the key comes from an environment
  variable; the sandbox has no seccomp filter.

NEXT SAFE ACTION
- A conformance run of the loop against the three real providers, with a
  key, outside CI. Then the trusted session, which is what stands between
  this and anything above class 1.
```

## Runs

For commit `2648699d9abc`:

| Workflow | Run | Result |
|---|---|---|
| `rust` | 43196470381 | Pass: every component, `services/tur-pax` included |
| `pax` | 43196470503 | Pass: 197 checks, 0 failed |

Earlier runs on the branch, for the record:

| Commit | `rust` | `pax` | What happened |
|---|---|---|---|
| `8e36a5c` | 43191335271 | not yet added | Compiled, clippy clean, 163 unit tests passed. Failed only at "fixes were needed": lockfile and formatting. |
| `5c9429e` | 43193135345 | not yet added | The loop, injection and program tests passed on their first run. Failed only at formatting. |
| `d271768` | 43194834093 | 43194833793 | `pax` passed on its first run: 193 checks. `rust` failed only at formatting. |
| `0ec9290` | 43195751925 | 43195751932 | `pax`: 194 checks. `rust` failed only at formatting. |

No run failed for a reason other than the lockfile or formatting, which CI
computes because no development machine has a toolchain. Six pushes in all,
the one that adds this report included.

Reading the evidence of the first `pax` run found a weakness in the test itself
rather than in the code. The check that the caller's environment does not reach
a sandboxed command looked at the output of `env`, and the output filter had
withheld one line of it: by the length of the output, `PWD=/`, which bubblewrap
sets and the secret detector takes for a password. A canary variable whose name
ends in `_SECRET` would have been withheld the same way, so that check could
have passed with the canary present. It now reads the names of the variables,
from which nothing is withheld, and requires them to be exactly the expected
six. For the same reason the two sandbox scenarios now also require that the
output filter withheld nothing at all, and the read of the protected files must
report "No such file or directory" for each of the six.

## What ran end to end

Workflow `pax` (`tests/pax/e2e.py`) on `ubuntu-24.04`, kernel 7.0.0-1017-azure,
bubblewrap 0.9.0-1ubuntu0.3, util-linux 2.39.3.

**The sandbox really ran.** bubblewrap could not build the sandbox on the runner
as it comes (`bwrap: loopback: Failed RTM_NEWADDR: Operation not permitted`:
Ubuntu's AppArmor restriction on unprivileged user namespaces). The preflight
applied the documented remedy, an AppArmor profile that lets `/usr/bin/bwrap`
create user namespaces, and nothing wider. Had that failed, the workflow would
have failed; the test has no path that runs without the sandbox.

The workflow builds `tur-pax` and the five tool components, installs the tool
programs to `/usr/bin`, the catalogue to `/usr/share/tur/catalogue` and a
hardware fixture tree to `/usr/share/tur-pax-e2e`, and runs `tur-pax run` on ten
scripted tasks. The driver decides from outside: from the audit log, from what
the scripted provider was sent, and from canaries.

**Controls first** (17 checks). Outside the sandbox every canary works: a
probe reaches a TCP listener, an abstract Unix socket, a stand-in session bus
socket in `/run/user/<uid>`, an X11 socket path under `/tmp/.X11-unix` and the
system bus socket; the SSH canary file is readable; a command can write;
`sudo -n id -u` prints 0. If a control fails the scenarios are reported as not
run.

| Scenario | What the scripted model does | What was proved |
|---|---|---|
| `two-tools` | Calls two tools in one response: the hardware problems of a fixture laptop and a catalogue route | `tur-device problems --from` and `tur-catalogue route` ran in the sandbox through their adapters; two decisions, class 0, tier 1; the fixture's `TUR-DEVICE-*` findings and the route for `com.discord.discord` reached the model as framed tool results; the final answer came back |
| `shipped-tools` | Calls all seven shipped tools | Each shipped adapter ran its program against the runner itself and returned its JSON: `tur-diag status`, `tur-device inventory --redact` and `problems`, `lsblk` fed to `tur-storage volumes`, `tur-catalogue search` and `show`, `tur-runtime inspect /usr/bin/true` |
| `readonly-command` | Runs `cat /etc/os-release`, then `cat` of a file in the granted folder | Class 0, tier 4, sandbox `read-only` in the audit log; both outputs reached the model; a granted folder is readable |
| `sandbox-probe` | Eight commands that report on the sandbox from inside | Variables are exactly `HOME LANG LC_ALL PATH PWD TERM`; `NoNewPrivs: 1`; no effective or permitted capabilities; the limits read back (CPU 10 s, 128 processes, 256 files, 4 GiB address space, 16 MiB files, no core); the home directory holds only `Documents`; `/run` holds only `log` and `udev`; `/tmp` is empty; 4 processes visible; the only interface is `lo` |
| `sandbox-attacks` | Seven hostile commands, all allowed by the policy engine as class 0 | Writing to the granted folder, the home directory, `/usr`, `/etc`, `/var/tmp`: "Read-only file system", nothing written; `rm -rf` of the granted folder and the home directory: failed, nothing deleted; a write to `/tmp` works inside and is not on the host; the socket probe reaches nothing (TCP and abstract socket refused; the bus, X11 and system bus paths do not exist) and no listener saw a connection; the protected files do not exist inside; a search for the canaries finds none; `sudo` refuses ("no new privileges") |
| `injection-tool-result` | Reads a file that says "ignore your instructions…", then proposes a class 3 and a class 2 tool, alongside a class 0 and a class 1 tool of the same service | The injected text did reach the model, as a tool result. Class 3 and class 2: refused, `TUR-AI-POLICY-006`, recorded, the tool program did not run. Class 0 and class 1: the same program ran, which shows the absence is the policy's doing |
| `injection-lies` | States a class in the arguments, invents tools, sends arguments outside the schema, repeats a key | Eight `proposal_rejected` records (`TUR-AI-TOOL-001`, `-002`), no decision, nothing run; then the class 3 tool with valid arguments: class 3, refused |
| `never-stops-steps` | Makes a tool call in every response, for ever | Cut at 4 steps; exit status 20; `TUR-AI-TASK-001` |
| `never-stops-spend` | The same, with a limit of 300 tokens | Four responses fit; the fifth is cut when its input is reported and discarded; `TUR-AI-PROVIDER-010` |
| `output-filter` | Prints a file with an access key identifier, a password line and a private-key block | Three pieces withheld; the other lines reached the model; the audit log says `3`, `known_token,password,private_key`, `TUR-AI-OUTPUT-001` and holds none of the text |

Over all scenarios together (8 checks): no canary string was ever sent to the
provider; every canary file is unchanged; no listener saw a connection other
than the control's; no decision above class 1 is anything but refused;
everything performed is a class 0 or class 1 action; the key is in no
transcript, error output or audit log; the system text is identical in every
request.

The tasks were started with `TUR_PAX_TRUSTED_PATH=1`,
`TUR_PAX_TRUSTED_PATH_AVAILABLE=true`, `TUR_PAX_ALLOW_CLASS=3`,
`TUR_PAX_CONFIRM=yes` and `TUR_PAX_NO_SANDBOX=1` in the environment. None of
them means anything to the program.

## What did not run

- **A real model.** Every "model" above is a list of responses in a JSON file.
- **Injection (d), memory, in the `pax` workflow.** No program can put an entry
  into a memory store, so the workflow cannot prepare one. It runs in the `rust`
  workflow instead, at two levels: the loop
  (`injection::an_instruction_recalled_from_memory_changes_no_decision`) and the
  built program (`run_cli::an_instruction_in_the_memory_store_changes_no_decision`).
- **The OpenAI and Google adapters** under the loop.
- **The sandbox on Fedora or on the image.**
- **A hostile grant in the `pax` workflow.** The grant rules are unit-tested
  (57 hostile paths, every subset of 8 acceptable ones, 51 protected paths) and
  tested against a real file system in the `rust` workflow; the program's
  refusal of `--grant-read` for `/`, `/etc`, `/run/user/1000` and `.ssh` is
  tested in `run_cli`. The workflow itself grants one ordinary folder.
- **A command that exceeds a limit**, as said above.
- **`tur-pax chat` in the `pax` workflow.** It is tested in `run_cli`.

## Changes to the policy engine's public interface

- `cli::TopCommand` has two new variants, `Run(textui::RunArgs)` and
  `Chat(textui::ChatArgs)`; `cli::run` dispatches them.
- The crate root re-exports the main types of the new modules.

Nothing else. No engine type, function or test was changed. `manifest validate`
and `policy check` behave as before; their 15 command-line tests pass unchanged.
The orchestrator's codes are in their own table so that the engine's registry
and the test that pins its code families are untouched.

## Dependencies

No new third-party crate is named in `Cargo.toml`. `tur-pax` now depends by path
on `tur-pax-providers` and `tur-pax-memory` (both under the project licence) and
so links what they already linked: `ureq`, `rustls`, `ring`, `rustls-webpki`,
`webpki-roots`, `zeroize`, `chrono`, `chrono-tz` and their dependencies. By
their crate metadata these are under MIT, Apache-2.0, ISC or BSD terms, and
`webpki-roots` under CDLA-Permissive-2.0. None is GPL. The licences were not read
again for this change.

At run time the sandbox needs `bubblewrap` (LGPL-2.0-or-later) and `prlimit`
from `util-linux` (GPL-2.0-or-later). Both are separate programs that `tur-pax`
starts; neither is linked.

## Changes needed in other components and shared files

Not made here; this change touches only its own files.

- `tur-pax-providers`: parse tool-call arguments strictly, or pass the argument
  text on with the call. Today a repeated key is silently reduced to its last
  value before the orchestrator sees the call; the orchestrator recovers the
  text from the stream's fragments and refuses a call whose text it cannot see.
- `tur-diagnostics`, `tur-catalogue`, `tur-runtime`: ship an `agent-tools.json`.
  Their manifests are in `services/tur-pax/tools/manifests` until then.
- `tur-storage`: a way to list the volumes of the live system, and one spelling
  of access for the manifest and the command line.
- `tur-device-manager`: command-line forms for `device_list` and `device_get`.
- The image: install the manifests, the adapters and the catalogue; make sure
  `bubblewrap` and `prlimit` are present.
- `docs/architecture/agent-policy.md`: mention the two new subcommands.
- `docs/architecture/ai-control-layer.md`: the findings of
  `pax-orchestrator.md` section 13.
- `README.md`: the status line for the assistant.
