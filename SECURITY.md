# Security Policy

## Status

UOS is pre-alpha. No release exists, and nothing in this repository should be
relied on to protect a real system or real data.

## Reporting a vulnerability

Please report security issues privately, not in a public issue.

Use GitHub's private reporting: on this repository, open the **Security** tab and
choose **Report a vulnerability**.

Include what you found, how to reproduce it, the affected component and commit, and
the impact you expect. You will receive an acknowledgement, and the issue will be
tracked privately until a fix is available.

## Scope

In scope: code and configuration in this repository, including the UOS services,
image definitions, installer, update and recovery paths, and the AI control layer.

Out of scope: vulnerabilities in upstream projects that UOS integrates. Report
those to the upstream project. If the way UOS configures or integrates an upstream
component creates the problem, that is in scope.

## Areas of particular interest

- Anything that lets an update, installer or recovery action destroy data or leave
  a system unbootable.
- Privilege escalation through a UOS service or its D-Bus interface.
- Leakage of identifiers or secrets into diagnostic bundles or logs.
- Ways for content shown on screen, or audio, to make the AI assistant take an
  action its user did not ask for, or to bypass a confirmation.
- Escape from an application isolation profile.
