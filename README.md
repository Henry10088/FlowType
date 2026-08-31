# FlowType

English | [简体中文](README.zh-CN.md)

> **Speak on your phone. Type anywhere on Windows.**

FlowType transforms your Android phone into a real-time voice input extension for Windows. Simply place your cursor anywhere—in VS Code, Codex, your browser, terminal, document, or chat app—and speak or type on your phone. Your words stream directly to your active Windows cursor with near-zero latency, completely eliminating the clunky "dictate on phone, copy, switch devices, paste" workflow.

FlowType seamlessly leverages your existing Android input method (Gboard, Sogou, iFlytek, etc.) without replacing your keyboard or bundling redundant speech-recognition engines.

## Why FlowType

Mobile voice input is fast, accurate, and natural—especially for long-form dictation and East Asian languages. However, bridging mobile speech input to a PC has traditionally been frustrating, requiring manual copy-pasting across devices that disrupts your focus and workflow.

Naive keystroke-forwarding tools fall apart with modern speech recognition. As speech-to-text models process audio in real time, they frequently revise earlier words based on context, delete filler words, or rewrite whole phrases on the fly. FlowType solves this by synchronizing the **full text state alongside monotonic sequence numbers**. The Windows host calculates precise Unicode diffs against previously accepted states, ensuring that in-flight edits, rewrites, and deletions accurately update target text without garbling.

What makes FlowType unique:

- **Native IME Experience**: Use the mobile keyboard and voice dictation you already love with zero learning curve.
- **Revision-Aware Synchronization**: Full-state snapshot diffing accommodates dynamic context corrections and rewrites in real time.
- **Direct Cursor Injection**: Injects text directly into your focused Windows application without intermediate clipboards or staging windows.
- **Direct & End-to-End Encrypted**: Communicates directly over local Wi-Fi or Tailscale via TLS 1.3. Your text stays strictly on your devices without public relay servers.

## Where It Fits

- **AI Prompts & Workflow**: Dictating complex, structured prompts into Codex, Claude, ChatGPT, or IDE assistants.
- **Content Creation**: Effortlessly drafting long emails, technical documentation, instant messages, notes, and code comments.
- **Multilingual Input**: Bringing the superior accuracy of mobile Asian/multilingual speech-to-text engines to Windows.
- **Ergonomics & Standing Desks**: Dictating comfortably away from your keyboard using your phone as a handheld wireless input pad.
- **Multi-Device Workflows**: Seamlessly pairing one phone with multiple Windows workstations.

## How It Works

```text
Android System Input Method
        -> Full Text Snapshot + Sequence ID
        -> LAN / Tailscale (WSS over TLS 1.3)
        -> Windows TSF Composition
        -> Active Foreground Application
```

1. **Pair Once**: Launch FlowType on Windows and scan the on-screen QR code from your Android device to establish a persistent, secure pairing.
2. **Focus Cursor**: Choose your target PC on the phone and place the Windows cursor in your destination app.
3. **Stream Input**: Speak or type using your mobile keyboard; input streams directly to your PC cursor in real time.
4. **Complete & Archive**: Tap "Done" to finalize input. Completed text is saved securely in your Android history for quick reuse.

Pairings persist until explicitly removed. A single phone can pair with multiple PCs, and the Windows desktop widget can prompt the phone to switch active devices on demand.

## Security & Privacy

### Key security assurances

| Question | Answer |
| --- | --- |
| Is my text protected on an untrusted LAN or Wi-Fi network? | **Yes.** Provided that the QR code was scanned from the intended PC and neither endpoint is compromised, TLS 1.3 keeps the content encrypted and detects tampering. |
| After correct pairing, can a router, gateway, or another device on the network impersonate my PC or phone? | **No, while the device keys remain protected.** Android accepts only the Windows public key pinned during QR pairing, and Windows requires a fresh signature from the paired phone on every connection. An intermediary can block the connection, but cannot silently read or alter accepted text. |
| What can the local network observe? | IP addresses, ports, connection timing, approximate traffic volume, and the mDNS online advertisement. It cannot see the input content. |
| Does input content pass through a FlowType public server? | **No.** Input travels directly between the phone and the selected PC over LAN or Tailscale. GitHub is contacted separately for update checks and downloads. |
| Where is completed text history kept? | Android stores it locally in encrypted form. FlowType does not create a completed-text history on Windows. |

### How the protection works

FlowType establishes trust out-of-band via QR code pairing. Local mDNS discovery is used strictly to detect whether already-paired PCs are online; it cannot initiate unauthorized pairings or override saved addresses and pinned public keys.

- **Encrypted Transport**: All communication is secured via TLS 1.3 with certificate public-key pinning established during pairing. Local network observers (such as routers or gateways) cannot inspect or tamper with your text.
- **Device Authentication**: The pairing QR code embeds a single-use token. Subsequent reconnections use fresh cryptographic challenges signed by a separate Android Keystore key for each PC. Hardware-backed storage depends on the Android device.
- **Local Key & Data Protection**: Android drafts and history are encrypted at rest using Keystore-managed AES-GCM keys. The Windows private key is protected by DPAPI. The elevated injection component has no network listener and holds no long-lived pairing keys.
- **Verified Updates**: Background updates are validated against separately signed release manifests, SHA-256 checksums, and native platform signatures before installation, reducing the risk of a modified manifest or package being accepted.
- **No Completed Windows History**: FlowType does not persist completed input text on Windows, and Injector diagnostics omit the text body.

> [!NOTE]
> **Security Boundaries**: These protections cannot defend against a compromised host or mobile OS, malicious third-party keyboards, or how target applications process received text. For full details on key lifecycle, privilege boundaries, and threat modeling, see the [Security Model](docs/security-model.md) ([中文](docs/security-model.zh-CN.md)). To report a security vulnerability, please refer to [SECURITY.md](SECURITY.md).

## Features

- **Multiline Live Streaming**: Real-time multiline text input powered directly by your native Android keyboard.
- **State-Aware Diffing & Resumption**: Full-state synchronization engine flawlessly handles mid-sentence corrections, large deletions, rewrites, and network reconnects.
- **Persistent Pairing & Multi-PC Support**: Quick QR-based setup with effortless switching between multiple paired Windows PCs.
- **Resilient Connection Handling**: Clear connection status with automatic, seamless reconnection—retaining mobile drafts even during network drops.
- **Native Windows Integration**: Replacement-safe composition through the Windows Text Services Framework (TSF), optimized for VS Code, Codex, browsers, terminals, and office suites.
- **Desktop Floating Widget**: Lightweight, draggable Windows overlay for connection monitoring, PC switching, and quick window access.
- **Mobile Floating Controls**: Optional Android floating bubble and compact input panel for system-wide access across apps.
- **Encrypted Mobile History**: Secure local history on Android with one-tap copy, reuse, and deletion.
- **Quick Image Transfer**: Push camera photos or gallery images directly to the Windows clipboard with optional original quality.
- **Native Bilingual UI**: Complete English and Chinese interfaces that automatically adapt to your system language.
- **Reliable Auto-Updates**: Background version checks, resumable downloads, cryptographic integrity verification, and user-confirmed installs.

## Installation

FlowType is in active pre-1.0 development. Windows and Android releases are versioned independently.

### Windows

1. Download `FlowType-<version>-x64-setup.exe` from [GitHub Releases](https://github.com/Henry10088/FlowType/releases).
2. Run the installer and accept the administrator (UAC) prompt to register the text services component, configure autostart, and set up firewall rules.
3. Launch FlowType from the Start menu; the main window will present a QR code for Android pairing.

### Android

1. Download `FlowType-<version>-android-release.apk` from [GitHub Releases](https://github.com/Henry10088/FlowType/releases).
2. Allow installation from unknown sources in Android settings, install the APK, and launch the app.
3. Scan the QR code displayed on Windows to complete pairing.

> Requirement: Android 10 (API 29) or later. Official release APKs are cryptographically signed; unsigned local builds are for development only.

## Current Limitations

- **Network Reachability**: Both devices must be on the same local network (LAN) or connected via Tailscale. For privacy reasons, no public relay server is provided.
- **Target Focus Lock**: FlowType locks onto the foreground window active when the session begins. Switching window focus will not automatically redirect input to avoid unintended text injection.
- **Remote Desktop (RDP)**: Running FlowType only on the local host cannot inject text through `mstsc.exe` into an RDP session. Install FlowType inside the remote machine and connect directly to it from your phone.
- **Elevated Permissions & Security Software**: Registering and repairing Windows input components requires administrator privileges. Certain endpoint security software or sandboxed applications may block low-level text injection.
- **OEM Background Restrictions**: Background retention, floating windows, camera access, and continuous voice recognition depend on Android OEM power-management policies and IME behavior; uninterrupted dictation while locked is not guaranteed.
- **Enter Key Behavior**: Line breaks are synchronized literally as newline characters; tapping "Done" does not send an extra Enter keypress.
- **Image Transfer Scope**: Currently supports transferring one image at a time to the Windows clipboard without automatic pasting.

> Note: Public cloud relays, guaranteed lock-screen dictation, batch image transfers, and automatic Enter keypresses are outside the current roadmap.

## Project Structure

```text
android/                    Android app, session state machine, and networking client
windows/flowtype-core/      Core protocol definitions, sequence state, and Unicode diff logic
windows/flowtype-app/       Windows main application, WSS server, and Win32 UI
windows/flowtype-injector/  Elevated text injection service and TSF coordinator
windows/flowtype-tip/       Windows Text Services Framework (TSF) component
protocol/v2/                Language-neutral protocol contracts and test fixtures
docs/                       Product requirements, architecture, security model, and roadmap
installer/                  Inno Setup packaging scripts
```

For deeper technical details, explore the [V1 Product Requirements](docs/requirements-v1.md), [V1 Architecture](docs/architecture-v1.md), [UI Architecture](docs/ui-architecture.md), and [Protocol v2 Specification](protocol/v2/README.md).

<details>
<summary><strong>Build from Source</strong></summary>

### Android Build Prerequisites

- JDK 17
- Android SDK Platform 36, Build Tools, and Platform Tools
- Android Gradle Wrapper 8.11.1 (included in repository)

```powershell
cd android
.\gradlew.bat test lint
.\gradlew.bat packageFlowTypeRelease
```

To generate a signed release APK, configure the following environment variables:

```text
FLOWTYPE_ANDROID_KEYSTORE
FLOWTYPE_ANDROID_STORE_PASSWORD
FLOWTYPE_ANDROID_KEY_ALIAS
FLOWTYPE_ANDROID_KEY_PASSWORD
```

> **Important**: Never commit signing keys or credentials to version control.

### Windows Build Prerequisites

- Windows 10 or 11 x64
- Visual Studio Build Tools with MSVC C++ toolchain and Windows SDK
- Rust Stable (MSVC toolchain)
- Inno Setup 6 (for packaging the installer)

```powershell
cd windows
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
rustup target add i686-pc-windows-msvc
cargo build --workspace --release
cargo build -p flowtype-tip --release --target i686-pc-windows-msvc
Copy-Item .\target\i686-pc-windows-msvc\release\flowtype_tip.dll .\target\release\flowtype_tip_x86.dll -Force
```

Once Windows release binaries are compiled, build the installer from the repository root:

```powershell
$tipHash = (Get-FileHash .\windows\target\release\flowtype_tip.dll -Algorithm SHA256).Hash.ToLowerInvariant()
$tipX86Hash = (Get-FileHash .\windows\target\release\flowtype_tip_x86.dll -Algorithm SHA256).Hash.ToLowerInvariant()
& "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe" `
  /DBuildDir="..\windows\target\release" `
  /DTipDllHash=$tipHash `
  /DTipDllX86Hash=$tipX86Hash `
  installer/flowtype.iss
```

Run `scripts\verify-version.ps1 -Platform Windows` or `-Platform Android` before publishing. Both platforms maintain independent version numbers and GitHub Releases.

</details>

## Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request. Make sure to run Android tests/lint and Windows tests/Clippy locally beforehand.

Development logs in `docs/validation/` serve as historical validation records and do not constitute full compatibility guarantees for current releases. See the [Validation Overview](docs/validation/README.md).

## Development Disclosure

FlowType was developed with assistance from OpenAI Codex as part of an
AI-assisted development workflow. The project maintainer is responsible for the product
requirements, architecture, implementation decisions, code review, testing,
security decisions, and releases.

AI assistance does not replace engineering review or security validation.

## License

FlowType is licensed under the [Apache License 2.0](LICENSE). Third-party dependencies are subject to their respective upstream licenses.
