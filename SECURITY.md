# Security policy

## Reporting a vulnerability

Please report security problems privately through GitHub:
[open a private security advisory](https://github.com/san-digital/seam-atlas/security/advisories/new).
Do not open a public issue for them. If the advisory form is not available, open an
issue that asks for a private contact, without any details of the problem.

Useful reports include the version, the policy file or input that triggers the problem,
and what you expected to happen.

We will acknowledge the report, keep you updated while we work on a fix, and credit you
in the advisory unless you prefer otherwise.

## What counts

`seam-atlas` reads policy files and prints results; it makes no network requests and
has no dependencies. The most serious problems are wrong answers: a `diff` that reports
two policies as equivalent when some input changes outcome, or a `cliffs` run that misses
a boundary. Please report those as security issues if a real policy could be affected.
Crashes or hangs on hostile policy files are also in scope.

## Supported versions

Fixes go into the latest release only.
