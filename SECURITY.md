# Security Policy

## Supported Versions

Only the latest `0.1.x` release and the current `main` branch receive
security fixes while the project is in V1 development.

## Reporting a Vulnerability

Do not disclose a suspected vulnerability in a public issue. Prefer a
private GitHub Security Advisory. Do not publish vulnerability details in
an issue, pull request, or discussion. If the advisory channel is
unavailable, contact the repository maintainer through GitHub and request
a private reporting channel before sharing details. Include:

- affected version or commit;
- Android model or Windows version, when relevant;
- reproduction steps or a minimal proof of concept;
- possible impact and any logs with secrets removed.

Please do not include pairing tokens, private keys, draft text, or
network captures containing personal content. Reports are acknowledged
when practical and are investigated before public disclosure.

## Security Scope

FlowType is designed for a trusted local or Tailscale network. Text is
sent over authenticated TLS, and device identities are stored in Android
Keystore or Windows current-user protected storage. This does not make
an untrusted endpoint or compromised operating system safe.
