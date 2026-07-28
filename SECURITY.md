# Security policy

## Supported versions

| Version | Supported |
| --- | --- |
| Latest release | Yes |
| Older releases | No |

## Reporting a vulnerability

Please do not open a public issue for a vulnerability, a leaked control token, or a report that
contains private learner data. Use the repository's
[private vulnerability report](https://github.com/shasan731/linuxpracticelab/security/advisories/new)
instead. Include:

- the affected version and Windows version;
- a minimal reproduction;
- the expected and actual isolation boundary;
- logs with tokens, usernames, and personal paths removed; and
- your assessment of impact.

You should receive an acknowledgement within seven days. Valid reports will be investigated,
fixed on a private branch, and disclosed with an appropriate release. There is currently no paid
bug-bounty program.

The intended trust boundaries and accepted risks are documented in the
[threat model](docs/security/threat-model.md).
