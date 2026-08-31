# FlowType Security Model

[简体中文](security-model.zh-CN.md)

This document describes the security properties implemented by FlowType protocol v2. It is an implementation guide, not a third-party security audit or a claim that a compromised phone or PC can be made safe.

## Key security assurances

| Question | Conclusion |
| --- | --- |
| Is input protected if the LAN, Wi-Fi, router, or gateway is not trusted? | **Yes, within the documented endpoint assumptions.** TLS 1.3 encrypts accepted text and images, while the QR-pinned Windows public key prevents an on-path device from substituting its own server certificate. |
| Can an intermediary impersonate a previously paired phone? | **No.** Windows requires a valid signature over a new random challenge on every connection. A copied phone public key can verify signatures but cannot create them. |
| What can an intermediary do? | It can observe connection metadata and mDNS presence, delay packets, or block the connection. It cannot silently decrypt or modify content accepted by FlowType. |
| Does input content depend on a public FlowType relay? | **No.** The content path is a direct connection between the selected phone and PC. GitHub access is separate and is used for software updates. |
| What is the main remaining trust boundary? | The phone, its selected input method, the PC, and the target application can access plaintext as part of providing input. A compromised endpoint is outside this model. |

## Security goals

FlowType is designed to provide these properties:

- Encrypt text and image content between the selected Android device and Windows computer.
- Authenticate the Windows computer scanned by the phone and reject a server whose key does not match the QR code.
- Authenticate a previously paired phone on every connection without placing an exportable phone private key in the app database.
- Reject replayed connection authentication and inconsistent text sequence reuse.
- Keep network handling and long-lived pairing keys out of the elevated Windows input process.
- Verify downloaded updates independently of the network path that delivered them.

The design assumes that the Android and Windows operating systems, the selected Android input method, and the target Windows application are trusted. The limitations section defines this boundary in more detail.

## Data path and trust boundaries

```text
Android system input method
        |
        v
FlowType Android app
        |
        |  WSS / TLS 1.3
        v
FlowType Windows app (normal user privileges)
        |
        |  restricted named pipe
        v
FlowType Injector (elevated, no network listener)
        |
        |  restricted TSF IPC
        v
Focused Windows application
```

FlowType has no public relay for input content. The phone connects directly to a selected PC over a reachable LAN or Tailscale address. GitHub is contacted separately for update checks and downloads; it is not part of the input data path.

## Pairing and the QR code

The Windows pairing QR code contains:

- the protocol version and Windows computer ID;
- one or more candidate WSS addresses;
- the SHA-256 fingerprint of the Windows TLS public key in SPKI form; and
- a cryptographically random 32-byte one-time pairing token.

The pairing token exists only in Windows process memory. It has no visible countdown, but it becomes invalid after the first successful pairing, when the pairing window is closed or cancelled, or when a new QR code is generated. It is not the permanent credential.

Pairing proceeds as follows:

1. Android scans the QR code and attempts the listed addresses.
2. During the TLS handshake, Android accepts only the server public key whose SPKI SHA-256 matches the QR code.
3. Android creates a P-256 (`secp256r1`) signing key for this PC in Android Keystore.
4. Windows sends a fresh random challenge. Android signs the challenge-bound authentication payload and sends its public key together with the one-time token.
5. Windows verifies the token and signature, then stores the phone ID and public key. The token is invalidated.
6. Android retains only the successful address for this binding; the other QR candidates are not kept for later automatic rotation. The phone and PC remain paired until either side removes the binding, application data is cleared, the operating system is reinstalled, or a required key is lost.

An unexpired QR code is sensitive: someone who obtains all of its contents may race the intended phone to complete one pairing. Keep the pairing window and QR code private. Possession of a QR code does not reveal the Windows TLS private key.

## Transport and Windows authentication

The Windows server uses `rustls` and permits TLS 1.3. It uses a self-signed certificate because FlowType connects to LAN and Tailscale IP addresses rather than a stable public DNS name.

Android does not rely on the system certificate-authority list or a hostname match for this connection. It compares the server certificate's SPKI SHA-256 with the pin obtained from the QR code. The Android hostname verifier therefore permits the IP address intentionally; server identity is established by the SPKI pin, not by the hostname.

This prevents a router or other on-path device from substituting its own certificate. A device on the network can observe connection metadata such as IP addresses, ports, timing, and approximate traffic volume. It can delay or block the connection, but it cannot read or modify accepted FlowType text or image frames without the pinned Windows private key.

Tailscale adds its own network-layer encryption when it is used. FlowType's TLS layer remains enabled on Tailscale and does not depend on Tailscale for application identity.

### Local discovery metadata

Windows advertises `_flowtype._tcp.local.` over mDNS so Android can show the online state of computers that are already paired. The advertisement exposes the PC ID, protocol version, address, and port to the local network. It is not an authentication mechanism.

Android ignores advertisements whose PC ID is not already in its binding database. A matching advertisement changes only the online indicator; it does not replace the saved endpoint, TLS pin, or phone binding. A local attacker can observe or spoof this presence signal and may make the online indicator inaccurate, but the attacker must still pass the pinned TLS handshake and phone authentication before FlowType accepts content.

## Phone authentication and replay protection

Android creates a separate Keystore P-256 signing key for each paired PC. The private key is non-exportable through the application API. FlowType does not require biometric approval on every signature, because automatic background reconnection must be possible. Whether the key is hardware-backed depends on the Android device.

Every WSS connection starts with a new random 32-byte nonce generated by Windows. Android signs this domain-separated byte sequence using `SHA256withECDSA`:

```text
flowtype-auth-v1\0{pc_id}\0{phone_id}\0{nonce}
```

The first pairing request also includes the one-time token and phone public key. Later connections send the phone identity and a new signature only. Windows verifies the DER-encoded ECDSA signature against the public key stored for that phone.

Android's `phone_id` is an opaque device-record identifier, not an authentication credential. An existing ID is retained when the phone is paired again. If no local ID exists, the app uses a namespaced SHA-256 hash of the Android device identifier, falling back to a random ID only when that platform value is unavailable. Windows performs an idempotent update keyed by `phone_id`, replacing the phone public key while retaining binding metadata; it never merges different devices solely because their display names match.

A phone public key is not secret and is not sufficient to impersonate the phone; an attacker must produce a valid signature using the corresponding private key. The fresh nonce prevents a captured authentication message from being replayed on a later connection. Removing a phone from Windows deletes its accepted public key, so subsequent authentication from that binding fails.

## Text and image integrity

All business messages are accepted only after TLS and phone authentication complete.

FlowType sends complete text snapshots rather than individual simulated keystrokes. Each active input contains a session ID and an increasing sequence number. Windows calculates the Unicode difference between accepted snapshots. Duplicate states are idempotent, while reuse of one sequence number with different text is rejected as a protocol error. On reconnection, Android can resume an active session with its last acknowledged sequence and latest complete snapshot.

This model is important for voice input: an Android input method may revise, replace, or delete previously recognized words. A complete snapshot allows Windows to converge on the phone's latest state instead of assuming that every recognition result is an append-only stream.

Image clipboard transfer uses the same authenticated WSS connection. Its header includes the byte length and SHA-256 digest. Windows verifies the digest, encoded format, byte limits, decoded dimensions, and pixel limit before replacing the clipboard.

For the protocol contract and message limits, see [FlowType protocol v2](../protocol/v2/README.md).

## Local key and content storage

### Android

- Each per-PC P-256 private key is held by Android Keystore and is not stored in the application database.
- Draft and history text fields are encrypted with `AES/GCM/NoPadding` using AES keys held by Android Keystore.
- The binding database contains application-private metadata such as PC name, selected address, computer ID, public-key pin, and phone public-key information. This metadata is not described as secret or separately field-encrypted.
- The one-time pairing token is cleared from the binding database after pairing succeeds.
- Clearing all history also deletes the history encryption key.
- Android application backup is disabled in the manifest.

Plaintext necessarily exists in application memory while text is displayed or synchronized. The selected input method may also maintain its own history or cloud behavior; that is controlled by the input method, not FlowType.

### Windows

- The Windows TLS identity private key is encrypted using Windows DPAPI for the current user before it is written to disk.
- The certificate, public key, computer ID, paired phone public keys, device names, and pairing timestamps are not secrets. They are stored under the current user's application data.
- Windows does not maintain a completed-input history. The active text exists in the main process and Injector memory while a session is being synchronized.
- When the user selects Sync at a new cursor, the in-process TIP reads only the immediately preceding span whose length equals the complete Android text, solely for an exact duplicate check. That span stays on the PC, is not persisted, and is never logged. FlowType does not scan the target document at other times.
- Injector diagnostics record process IDs, sequence numbers, text lengths, and errors, but not the input text itself.

DPAPI protects the private key at rest within the Windows user boundary. It does not protect against an attacker already running with that user's credentials or administrator access.

## Windows privilege boundary

FlowType separates the network-facing application from the component that needs elevated input access:

- `flowtype.exe` runs with normal user privileges and owns the UI, WSS server, TLS identity, pairing records, and protocol state.
- `flowtype-injector.exe` runs elevated. It has no network listener and does not hold the long-lived TLS or phone pairing keys.
- In installed builds, the Injector is launched through a registered scheduled task rather than accepting an arbitrary elevated command line.
- The main app and Injector communicate over the `flowtype-input-v5` named pipe. Its DACL permits only the current user, Administrators, and SYSTEM.
- The Injector checks the client process ID and installed main-program path. The main app checks the server process path, reported binary path, IPC version, instance ID, and elevated state.
- IPC supports a fixed set of message types with a 1 MiB limit. It does not expose arbitrary command, script, or path execution.
- The Injector and the target-process Text Services component use a separate `flowtype-tip-v4` IPC channel and reject mismatched IPC versions.

These controls reduce the exposed elevated surface. They are not a sandbox and do not attempt to defend against a Windows administrator, who is already inside the operating-system trust boundary.

## Update integrity

Windows and Android fetch update metadata and packages from GitHub Releases. Installation does not rely on HTTPS alone:

1. The client verifies a separately signed P-256 update manifest using a public key embedded in the app.
2. It checks the downloaded file size and SHA-256 against that signed manifest.
3. Windows additionally verifies the installer Authenticode identity. Android verifies that the APK signing certificate matches the installed FlowType application.
4. Installation requires user confirmation through the normal platform flow.

Update signing uses a key separate from device-pairing identities and platform package-signing keys. See [Online update design](update-design-v1.md) for the release and recovery design.

## What this model does not protect against

FlowType does not claim to protect input content from:

- an attacker with Android root access, Windows administrator access, or equivalent control of either endpoint;
- a malicious or compromised Android input method;
- software that can read the screen, accessibility content, clipboard, target-application memory, or the target document;
- a person who can replace the QR code before it is scanned or obtain a still-valid pairing QR code and win the pairing race;
- the selected Windows application, which necessarily receives the input text;
- denial of service, including blocked traffic, unavailable networks, powered-off routers, or an application that refuses text injection.

FlowType also cannot hide connection metadata or its mDNS presence advertisement from the local network. Its goal is confidentiality and integrity of accepted content, device authentication, and a narrow elevated Windows interface.

## Recommended user practices

- Scan the QR code directly from the intended PC and close the pairing page when finished.
- Remove a phone or PC binding when a device is lost, transferred, or no longer trusted.
- Keep Android, Windows, the selected input method, and FlowType updated.
- Install release packages only from the project's [GitHub Releases](https://github.com/Henry10088/FlowType/releases) page.
- Treat text entered into terminals, administrator prompts, password fields, and remote systems according to those systems' own security requirements.

To report a vulnerability, follow [SECURITY.md](../SECURITY.md) and do not include keys, pairing QR codes, private input text, or sensitive packet captures in a public issue.
