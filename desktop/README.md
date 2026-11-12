<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# desktop/

TUR's desktop programs, themes and defaults. The design is
[desktop-experience.md](../docs/architecture/desktop-experience.md); the
decision is [ADR-0016](../docs/adr/0016-desktop-and-user-interface.md). How to
build on the shared kit is in
[desktop-kit.md](../docs/architecture/desktop-kit.md).

## The rules (design, section 4)

Everything TUR writes for the desktop is on one of three sides.

| Side | What | Licence | Where |
|---|---|---|---|
| A | Standalone programs | The project licence | `kit/`, `apps/`, `ci/probe/` |
| B | Configuration and theme data; no code | The project's own work | `data/` |
| C | Code a GPL program loads and runs | The host's licence | `upstream-licensed/<host>/`; empty |

For a program on side A:

- **It is a `QGuiApplication`, never a `QApplication`.** A `QApplication`
  loads the widget style, which under Plasma is Breeze and GPL-only.
- **It links and imports only what [`ALLOWED_IMPORTS`](ALLOWED_IMPORTS)
  lists.** Adding a line means the library's licence was checked, file by
  file where a project mixes licences. Nothing on the forbidden list is added
  without an ADR: the GPL-only Qt modules, Kirigami Addons, `org.kde.ksvg`,
  the KIconThemes QML plugin, KIO, libplasma and `org.kde.plasma.*`, Purpose.
- **It is not started from a copy of an upstream file.**
- **It uses public interfaces**, and where one is KDE's own the use is
  confined to one program.
- **Qt's tools are run, not linked**, and QML is not compiled ahead of time to
  C++ (`NO_CACHEGEN` on every QML module) until the legal review has answered
  the question about generated code.
- **Every file starts with the SPDX identifier**
  `LicenseRef-TUR-Noncommercial-1.0.0`, in the comment syntax of its language.
  JSON has no comments: put the identifier in a string in the first lines.
  Reference images (`.png`) are exempt.
- **Every colour comes from a token.** No colour literal in QML outside the
  two generated token files. Blue is the system; gold is P@x and is reachable
  only through the module `Tur.Kit.Pax`, which only P@x's surfaces may import.

`ci/boundary.py` enforces all of this on every change, and with `--linked`
checks what the built programs really link; `ci/boundary.py --self-test`
plants one violation of each kind and requires that each is caught.

## Layout

```text
CMakeLists.txt, CMakePresets.json   one CMake project; presets ci and ci-sanitize
ALLOWED_IMPORTS                     the data of the boundary check
kit/                                tur-kit: QML module Tur.Kit and libturkit
  tokens/ controls/ message/        the kit itself
  pax/                              Tur.Kit.Pax: gold, for P@x's surfaces only
  a11y/                             Tur.Kit.Testing: the test harness
  gallery/                          tur-kit-gallery: every component, every state
  tests/                            QML tests, unit tests, the screenshot renderer
  settings/ fixtures/               later tasks (D-07 and the fixture backend)
apps/tur-centre/                    tur-centre, the application centre, part 1 (D-10)
  src/ qml/ tests/                  the program; its unit and QML tests; shots/
  fixtures/                         recorded answers of tur-catalogue; see below
apps/ dbus/ data/ packaging/        later tasks; see the README in each
upstream-licensed/                  side C: empty but for a README
ci/                                 the builder image, the scripts CI runs, the toolchain proof
tests/screenshots/                  reference images
tests/e2e/ tests/compositor/        later tasks (D-05, D-15)
```

## Building and testing

Nothing here is compiled or run on a development machine (AGENTS.md). Push,
and the `desktop` workflow does it:

| Job | What it does |
|---|---|
| `boundary` | The licence boundary; the tokens and their contrast table |
| `builder` | Builds the builder image if it is not in the cache |
| `build-test` | Configure, build (warnings are errors), what the binaries link, unit and QML tests headless, `qmllint` (a warning fails), the toolchain probe, then the tests again under AddressSanitizer and UndefinedBehaviorSanitizer |
| `screenshots` | Renders every gallery page and every state of the application centre twice, compares with the references, uploads everything |

```sh
git push -u origin HEAD
scripts/dev/ci.py wait
scripts/dev/ci.py artifact RUN_ID desktop-build-test DIR    # logs, the probe's report and pictures
scripts/dev/ci.py artifact RUN_ID desktop-screenshots DIR   # actual/, reference/, diff/, proposed/, sheets
```

Inside the builder the steps are `ci/build.sh ci`, `ci/test.sh ci`,
`ci/lint.sh ci`, `ci/screenshots.sh ci`, each a few lines around `cmake
--preset`, `ctest --preset` and the `all_qmllint` target.

## Reference images

A reference image says "this picture is what the design intends". The
`screenshots` job can only say that nothing changed since.

**How a picture is made.** `kit/tests/shots/render_gallery.qml` loads each
gallery page and saves Qt Quick Test's `grabImage` of it, under the offscreen
platform and Qt Quick's software renderer, with animation frozen, the Basic
style, Inter at 10 pt and 96 dpi. Each page is rendered dark and light,
left-to-right and right-to-left with strings 40 % longer, at 1x, and dark and
light at 1.5x. Names are `<page>-<variant>-<direction>-<scale>.png`.

**How it is compared.** `ci/compare_screenshots.py`. Everything is rendered
twice and the two runs must be identical. Against the reference: the same
size, and no pixel differing by more than 8 of 255 in any channel. That
tolerance absorbs rounding at anti-aliased edges and nothing a person could
see; zero pixels may exceed it. It is not to be loosened to make a change
pass. Before it compares anything the job runs `compare_screenshots.py
--self-test`, which makes pictures that match and pictures that do not (a
missing hairline, one wrong pixel) and requires each verdict.

**How a reference is made or changed.** Nobody can render the pictures
locally, so CI makes them and a person or an agent accepts them:

1. Push the change. The `screenshots` job fails and says which pictures are
   new or different.
2. Fetch and look:

   ```sh
   desktop/ci/update-references.py RUN_ID --keep /some/dir/outside/the/repo
   ```

   This copies the run's `proposed/` pictures into `tests/screenshots/` and
   leaves the whole artifact in the directory: `actual/`, `reference/` (the
   old ones), `diff/` (what changed, in magenta) and a contact sheet
   (`contact-*.png`) for each combination.
3. **Look at every picture it lists**, in both variants. Clipped text, a
   missing glyph, text that cannot be read against its background, an empty
   area: any of these means the change is wrong, not the reference.
4. Commit the pictures with the change that caused them and push. The job
   must now pass, and the reviewer sees the new pictures in the pull request.

A change that should not alter any picture and does is a regression.

## Fixtures

A program is built, tested and photographed before the services it talks to
exist, and without them in the builder: it answers from recorded answers
instead (design, 15.2). `apps/tur-centre/fixtures/` is the first set: the
output of `tur-catalogue ... --json` for seed entries of the catalogue.

**Nobody writes a fixture by hand.** A test in the `rust` workflow,
`services/tur-catalogue/tests/desktop_fixtures.rs`, runs the catalogue program
for every recorded question and fails when a file differs from what it
prints. A picture made from the fixtures therefore shows what the router
really says. Nobody can run the router on a development machine either, so
when the fixtures must change, CI makes them:

```sh
git push                                   # the test fails, and prints the files as they should be
desktop/apps/tur-centre/fixtures/update.py JOB_ID   # the job `services/tur-catalogue` of the rust workflow
git diff desktop/apps/tur-centre/fixtures  # look at what changed, and why
```

Then accept the pictures that changed with them, as above. How the files are
shaped, and what the application centre does with them, is in
[tur-centre.md](../docs/architecture/tur-centre.md), section 4.

## The builder image

`ci/builder/Containerfile`: Fedora 45, the same release as the OS image, with
Qt 6, Kirigami, `qqc2-breeze-style`, the build tools and the fonts.

- **Pinned:** the base image, by digest, in `ci/builder.lock`.
- **Not pinned, and printed:** the packages. Fedora keeps only the newest
  update of a package, so an exact version cannot be asked for. The image is
  built once and kept in the Actions cache under a key made from the
  Containerfile and `builder.lock`; every later run loads that same image. A
  weekly run keeps the cache from expiring.
- **Checked:** `ci/builder.packages` lists the versions of Qt, the text stack
  and the fonts in the image the references were made with. `ci/versions.sh`
  compares and warns on a difference.

The cache can still be lost (it is evicted when the repository's caches grow
too large). The image is then rebuilt with the day's packages. If a picture
changes because Qt or a font did, regenerate the references as above and
update `builder.packages`, in a pull request of its own. To take newer
packages on purpose, raise `BUILDER_REVISION` in `builder.lock`.

The design asks for an image in the project's registry, pinned by digest.
That is the better end state and is not done: see
[desktop-kit.md](../docs/architecture/desktop-kit.md), "Known gaps".
