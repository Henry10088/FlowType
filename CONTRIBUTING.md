# Contributing

Thanks for helping improve FlowType. Keep changes focused and preserve
the input-session contract: Android sends complete, sequenced text
snapshots; Windows computes the injection transition.

## Development Environment

- Windows 10/11 x64 with Visual Studio Build Tools and the MSVC Rust toolchain
- Rust stable and Cargo
- JDK 17
- Android SDK Platform 36 and an Android API 29+ device or emulator
- PowerShell for the Windows packaging scripts

Do not commit signing material, APK/EXE outputs, private keys, pairing
tokens, or real input text. Local signing files belong under `signing/`.

## Checks Before A Pull Request

From `android/`:

```powershell
.\gradlew.bat test lint
```

From `windows/`:

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

From the repository root, verify that the Android, Windows, installer, and
documentation versions agree:

```powershell
.\scripts\verify-version.ps1
```

Changes to installer or release behavior should also run the Windows
release build and compile `installer/flowtype.iss` with Inno Setup.

## Change Guidelines

- Keep protocol and session changes covered by focused tests.
- Keep UI code separate from network, persistence, and injector logic.
- Prefer the existing Android Views and Win32 Common Controls patterns.
- Explain compatibility or security impact in the pull request.
- Use a short imperative commit subject, for example `fix: preserve latest snapshot on reconnect`.

Pull requests should describe what changed, how it was tested, and any
known limitations. Screenshots are useful for user-facing UI changes.
