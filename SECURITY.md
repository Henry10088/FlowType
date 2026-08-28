# Security Policy

FlowType's implemented trust model, pairing flow, key storage, Windows
privilege boundary, and explicit threat assumptions are documented in the
[Security model](docs/security-model.md) and its
[Chinese translation](docs/security-model.zh-CN.md).

## Supported Versions

The latest published Windows release, the latest published Android release,
and the current `main` branch receive security fixes during pre-1.0
development. Older releases may not receive backported fixes. Because the two
platforms are versioned independently, their latest version numbers may differ.

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

Please do not include pairing tokens, private keys, pairing QR codes, draft
text, or network captures containing personal content. Reports are
acknowledged when practical and are investigated before public disclosure.

## Security Scope

FlowType connects a phone directly to a selected PC over a reachable LAN or
Tailscale address. Accepted text and images travel over TLS 1.3 with pinned
Windows identity and challenge-response phone authentication. Android private
keys are held by Android Keystore, while the Windows TLS private key is
protected at rest with current-user DPAPI.

These controls protect the connection and stored application keys within the
documented trust boundaries. They do not make a malicious input method,
untrusted target application, rooted phone, or compromised Windows system safe.
