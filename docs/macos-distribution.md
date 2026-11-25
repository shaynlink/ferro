# Direct macOS distribution

Ferro packages direct macOS distribution for Apple Silicon. It is a Developer ID and notarization
workflow, not an App Store workflow: it does not enable the sandbox or provide App Store
entitlements.

## Project metadata and assets

New projects use schema 2 in `ferro.toml`:

```toml
schema = 2

[app]
name = "Atlas"
id = "dev.example.atlas"
targets = ["macos"]
version = "1.2.3"
build_number = 42

[macos]
signing_identity = "Developer ID Application: Example, Inc. (TEAMID)"
icon = "assets/macos/AppIcon.icns"
resources_dir = "assets/macos/resources"
```

`app.version` becomes `CFBundleShortVersionString` and `app.build_number` becomes
`CFBundleVersion`. Supply a real ICNS file at `icon`; static resources are recursively copied to
`Contents/Resources`. Symlinks, frameworks, plug-ins and executable resources are outside this
phase.

## Local release

Install a Developer ID Application certificate in the login keychain, then store a Notary profile
with Apple’s tooling:

```sh
xcrun notarytool store-credentials ferro-notary
ferro package macos --arch arm64 --notarize --notary-profile ferro-notary
```

The command compiles `aarch64-apple-darwin` with an Apple Silicon baseline, signs with hardened
runtime and timestamp, verifies the signature and architecture, creates a DMG, submits it with
`notarytool`, then staples and validates the returned ticket. Use `--format zip` for a ZIP instead;
Ferro staples the app before recreating that archive. `--output <directory>` changes the default
`dist/macos` destination.

Do not place certificates, private keys, Apple IDs, app-specific passwords or Notary API keys in
`ferro.toml` or source control.

## GitHub Actions example

Use repository secrets for an encoded Developer ID certificate, its password, and a Notary API-key
profile setup. Import them into a temporary keychain, invoke the same CLI command, then delete the
keychain in an `always()` step. The release job belongs in an application repository, not Ferro’s
ordinary pull-request CI, because notarization needs Apple credentials.

```yaml
- run: ferro package macos --arch arm64 --notarize --notary-profile ferro-notary
  env:
    # Create the temporary keychain and profile from GitHub Secrets in prior steps.
    CI: true
```

Before publishing, test the stapled artifact on a clean Mac. Mac App Store submission is a separate
future phase: it requires a sandbox, explicitly reviewed entitlements and Apple validation.
