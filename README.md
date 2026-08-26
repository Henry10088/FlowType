# FlowType

English | [简体中文](README.zh-CN.md)

> Speak into your phone. Your PC does the typing.

Use your phone's voice input to type on Windows in real time. FlowType uses the current Android input method, so voice typing and the regular keyboard both work. The Windows app sends the text to the active application.

## Current Version

The current version is `0.2.0` and remains an internal V1 release. Its data path is:

```text
Android input method -> local WSS connection -> Windows input location
```

Android sends complete text snapshots with increasing sequence numbers. Windows calculates the difference between snapshots, which lets it handle corrections, deletions, and replacements made by voice recognition.

## Features

- Multiline Android input using the current system input method
- Pair and switch between multiple Windows computers
- QR-code pairing with persistent device bindings
- Encrypted WSS transport and device authentication
- Automatic reconnection while retaining the latest complete draft
- Unicode text input for VS Code, Codex, browsers, terminals, and other Windows apps
- A movable Windows floating ball for connection status and quick computer switching
- Android input history with copy, reuse, and delete actions
- Optional Android floating input ball and panel
- Send one camera or gallery image to the Windows clipboard, with optional original quality
- Chinese and English interfaces selected from the system language
- Background update checks, resumable downloads, package verification, and user-confirmed installation

Public relays, continuous voice input while the phone is locked, multiple-image transfers, and automatic Enter input are outside the V1 scope.

## Installation

### Windows

Download `FlowType-<version>-x64-setup.exe` from [Releases](https://github.com/Henry10088/FlowType/releases), run it, and approve the administrator prompt. The installer registers the FlowType input service, enables startup with Windows, and adds the local-network firewall rule.

After installation, launch FlowType from the Start menu or run:

```text
flowtype.exe --show
```

### Android

Download `FlowType-<version>-android-release.apk` from [Releases](https://github.com/Henry10088/FlowType/releases). Allow installation from that source in Android settings, then install the APK. On first launch, scan the QR code shown by the Windows app.

The Android application ID is `app.flowtype`. Users migrating from the older package must uninstall it, install the current APK, and pair their computers again.

Release APKs are signed. Without local signing credentials, Gradle produces an unsigned internal build that must not be distributed as a release.

## Requirements

### Android

- JDK 17
- Android SDK Platform 36
- Android SDK Build Tools and Platform Tools
- Android Gradle Wrapper 8.11.1, included in the repository
- Android API 29 or later

### Windows

- Windows 10 or 11 x64
- Visual Studio Build Tools with the MSVC C++ toolchain and Windows SDK
- Stable Rust MSVC toolchain
- Inno Setup 6 for building the installer

## Local Build

### Android

From the repository root:

```powershell
cd android
.\gradlew.bat test lint
.\gradlew.bat packageFlowTypeRelease
```

The output is written to `android/app/build/outputs/apk/release/`. Configure all of the following variables to create a signed release APK:

```text
FLOWTYPE_ANDROID_KEYSTORE
FLOWTYPE_ANDROID_STORE_PASSWORD
FLOWTYPE_ANDROID_KEY_ALIAS
FLOWTYPE_ANDROID_KEY_PASSWORD
```

Never commit signing material.

### Windows

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

The main release files are:

```text
flowtype.exe
flowtype-injector.exe
flowtype_tip.dll
flowtype_tip_x86.dll
```

### Windows Installer

After building the Windows release files, run Inno Setup from the repository root:

```powershell
$tipHash = (Get-FileHash .\windows\target\release\flowtype_tip.dll -Algorithm SHA256).Hash.ToLowerInvariant()
$tipX86Hash = (Get-FileHash .\windows\target\release\flowtype_tip_x86.dll -Algorithm SHA256).Hash.ToLowerInvariant()
& "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe" `
  /DBuildDir="..\windows\target\release" `
  /DTipDllHash=$tipHash `
  /DTipDllX86Hash=$tipX86Hash `
  installer/flowtype.iss
```

The installer is written to `installer/output/`.

## Repository Layout

```text
android/                 Android app, session state, and network client
windows/flowtype-core/   Protocol, sequence state, and Unicode diff logic
windows/flowtype-app/    Windows app, WSS server, and Win32 UI
windows/flowtype-injector/ Elevated input service and TSF injection
windows/flowtype-tip/    Windows Text Services component
protocol/v1/             Protocol fixtures
docs/                    Requirements, architecture, plans, and validation notes
installer/               Inno Setup installer
```

See [UI architecture](docs/ui-architecture.md), [V1 requirements](docs/requirements-v1.md), and [V1 architecture](docs/architecture-v1.md) for the current technical baseline.

## Known Limitations

- Both devices must be on the same local network or a mutually reachable Tailscale network.
- Windows only updates the original foreground target; it does not take over a newly focused window.
- Floating windows, background behavior, and camera permissions vary across Android vendors and input methods.
- Continuous voice input while the phone is locked is not guaranteed.
- The Windows input service requires administrator permission to install and repair.
- Image transfer currently supports one image at a time.
- FlowType does not send Enter automatically, and finishing an input does not alter the target text.
- Run `scripts\verify-version.ps1` before a release to verify that Android, Windows, installer, and documentation versions agree.

## Security and Privacy

Text is transferred over TLS with device authentication. Android keys are stored in Android Keystore. Windows keys and pairing data are protected for the current Windows user. Completed text history remains on Android; Windows does not create its own history of completed text.

Read [SECURITY.md](SECURITY.md) before reporting a vulnerability. Do not post keys, pairing QR codes, input text, or sensitive packet captures in a public issue.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) before submitting changes. Run the Android tests and Lint checks, plus the Windows tests and Clippy checks, before opening a pull request.

## License

FlowType is licensed under the [Apache License 2.0](LICENSE). Third-party dependencies remain subject to their own licenses.

## Historical Validation Notes

Files under `docs/validation/` are development records, not complete compatibility guarantees for the current release. Some older notes use historical executable names. See the [validation notes overview](docs/validation/README.md) for context.
