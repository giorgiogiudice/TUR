# Contributing to TUR

TUR is at an early stage and needs help of every kind. The most useful
contributions right now are review of the [specification](docs/architecture/specification.md)
and of open [decision records](docs/adr/README.md).

The same rules apply to people and to automated agents. Agents must also follow
[AGENTS.md](AGENTS.md).

## Before you start

- Read the section of the specification that covers your area.
- Check `docs/adr/` for a decision that already settles the question.
- For anything larger than a small fix, open an issue first and describe the
  problem you want to solve.

## Making a change

1. Branch from `main`. Keep the change focused on one work item.
2. Include tests and the exact command that reproduces them.
3. Add or update an ADR if the change affects an interface, boot, storage,
   security or update behaviour, or departs from the specification. Use the
   [template](docs/adr/0000-template.md).
4. Open a pull request that states:
   - the problem and the component it belongs to;
   - what changed and why;
   - how it was tested, and what was not tested;
   - failure modes and how to roll back.
5. A change that can affect boot or storage must pass a boot test in CI and include
   a fault-injection or recovery test.

## Standards

- **Upstream first.** Prefer integrating a maintained project to writing a new
  one. Send generic fixes upstream.
- **Pinned dependencies.** Record the version or commit, the licence and the reason
  for every new dependency.
- **Evidence.** Compatibility and performance statements need a reproducible test
  and recorded hardware, versions and settings.
- **Privacy.** Never add silent data collection. Never log secrets or personal
  identifiers.
- **Honesty.** State known gaps. Do not describe something as working unless it was
  built and tested.

## Commit messages

Use a short imperative subject line, then a body that explains why the change was
made. Reference the issue or task ID where one exists.

## Licence

TUR is free for people to use for themselves. Its own code and documentation are
under the [TUR Noncommercial License](LICENSE.md), which does not permit business
or government use. By contributing you agree that your contribution to the
project's own code and documentation is licensed under those terms.

One exception matters. A change that modifies an upstream project, or that links
into one whose licence requires it (for example a patch to Wine or Darling, a KWin
or Plasma plugin, or a kernel module), must be contributed under that upstream
project's licence, and is kept in a clearly marked directory. See
[ADR-0003](docs/adr/0003-licence.md) and
[ADR-0010](docs/adr/0010-licence-excludes-government.md).

Do not contribute code you do not have the right to license, and do not copy code
from Microsoft, Apple, Google or other proprietary sources.

## Security issues

Do not open a public issue for a vulnerability. See [SECURITY.md](SECURITY.md).
