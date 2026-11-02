# Security Policy

This is a development port, not a claim of production readiness. See [PORTING.md](PORTING.md) for implemented boundaries, validation evidence and remaining gaps.

# Security

Cashew is content-addressed infrastructure code. Please treat parser bugs,
resource-bound bypasses, CID verification failures, and proof-validation issues
as security-sensitive.

Report suspected vulnerabilities through GitHub private security advisories for
this repository. If that is unavailable, contact the maintainers through the
project's private coordination channel before publishing details.

Please include:

- affected version or commit
- reproduction steps or proof input
- expected behavior and observed behavior
- impact assessment if known

Do not open a public issue for an active vulnerability before maintainers have
had a chance to investigate and coordinate a fix.


Use exact reviewed revisions. Tests and finite formal observations are evidence
for their documented scope, not proof of complete platform security.
