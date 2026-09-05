# Code signing the installers

The release pipeline currently ships **unsigned** installers, so first launch shows a
publisher warning (Windows SmartScreen, macOS Gatekeeper). Signing removes that friction
but requires paid identities that live in the release environment, not the repo. This is
the concrete recipe to wire it into `.github/workflows/release.yml` once you have them.

Do this **with the certificates in hand** so each step can be verified in a real release —
adding untested signing steps to the working pipeline blind is how releases break.

## What you need

| Platform | Identity | Cost / source |
| --- | --- | --- |
| Windows | Authenticode code-signing certificate (OV or, to skip SmartScreen reputation, EV) | a CA (DigiCert, Sectigo, …); EV usually ships on an HSM/token or via a cloud signing service |
| macOS | "Developer ID Application" cert + an app-specific password (or App Store Connect API key) for notarization | Apple Developer Program ($99/yr) |
| Linux | none required | AppImage/deb are conventionally unsigned; publish the `SHA256SUMS` (already done) |

## GitHub secrets to add

- `WINDOWS_CERT_BASE64` — the `.pfx` file, base64-encoded — and `WINDOWS_CERT_PASSWORD`.
  (For EV/cloud signing, use the vendor's action/credentials instead of a local `.pfx`.)
- `MACOS_CERT_BASE64` (Developer ID `.p12`, base64) and `MACOS_CERT_PASSWORD`.
- `MACOS_NOTARY_APPLE_ID`, `MACOS_NOTARY_PASSWORD` (app-specific password), `MACOS_NOTARY_TEAM_ID`.

## Windows (in the `windows-latest` matrix leg, after the build, before/along packaging)

Sign the app `.exe`, then let `cargo packager` build the NSIS installer, then sign the installer too.

```yaml
      - name: Sign Windows binaries
        if: runner.os == 'Windows' && env.WINDOWS_CERT_BASE64 != ''
        shell: pwsh
        env:
          WINDOWS_CERT_BASE64: ${{ secrets.WINDOWS_CERT_BASE64 }}
          WINDOWS_CERT_PASSWORD: ${{ secrets.WINDOWS_CERT_PASSWORD }}
        run: |
          $pfx = "$env:RUNNER_TEMP\cert.pfx"
          [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERT_BASE64))
          $signtool = (Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" | Select-Object -Last 1).FullName
          # Sign the app binaries produced by `cargo build --release`.
          Get-ChildItem target\release\forge_ide.exe, target\release\forge.exe | ForEach-Object {
            & $signtool sign /f $pfx /p $env:WINDOWS_CERT_PASSWORD /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 $_.FullName
          }
```

Add a second, identical `signtool sign` step **after** packaging that targets the NSIS
installer under `target/release/` (`forge_ide_*_x64-setup.exe`). `cargo packager` has no
built-in signing hook, so sign the produced installer as a follow-up step.

## macOS (in the `macos-14` leg)

Import the Developer ID cert into a temporary keychain, codesign the built app with the
hardened runtime, let `cargo packager` build the `.dmg`, then notarize and staple it.

```yaml
      - name: Import signing certificate
        if: runner.os == 'macOS' && env.MACOS_CERT_BASE64 != ''
        env:
          MACOS_CERT_BASE64: ${{ secrets.MACOS_CERT_BASE64 }}
          MACOS_CERT_PASSWORD: ${{ secrets.MACOS_CERT_PASSWORD }}
        run: |
          KEYCHAIN="$RUNNER_TEMP/signing.keychain-db"
          security create-keychain -p actions "$KEYCHAIN"
          security set-keychain-settings -lut 21600 "$KEYCHAIN"
          security unlock-keychain -p actions "$KEYCHAIN"
          echo "$MACOS_CERT_BASE64" | base64 -d > "$RUNNER_TEMP/cert.p12"
          security import "$RUNNER_TEMP/cert.p12" -k "$KEYCHAIN" -P "$MACOS_CERT_PASSWORD" -T /usr/bin/codesign
          security list-keychains -d user -s "$KEYCHAIN" login.keychain-db
          security set-key-partition-list -S apple-tool:,apple: -s -k actions "$KEYCHAIN"

      - name: Notarize and staple the DMG
        if: runner.os == 'macOS' && env.MACOS_CERT_BASE64 != ''
        env:
          MACOS_NOTARY_APPLE_ID: ${{ secrets.MACOS_NOTARY_APPLE_ID }}
          MACOS_NOTARY_PASSWORD: ${{ secrets.MACOS_NOTARY_PASSWORD }}
          MACOS_NOTARY_TEAM_ID: ${{ secrets.MACOS_NOTARY_TEAM_ID }}
        run: |
          DMG=$(ls target/release/*.dmg | head -1)
          xcrun notarytool submit "$DMG" --apple-id "$MACOS_NOTARY_APPLE_ID" \
            --password "$MACOS_NOTARY_PASSWORD" --team-id "$MACOS_NOTARY_TEAM_ID" --wait
          xcrun stapler staple "$DMG"
```

Give `cargo packager` the signing identity so the app inside the DMG is signed before the
DMG is built — set `[package.metadata.packager.macos] signingIdentity = "Developer ID Application: …"`
(or codesign `target/release/bundle/macos/*.app --options runtime --deep --sign "$IDENTITY"`
before the DMG step). Notarization requires the hardened runtime, so the app must be signed
with `--options runtime`.

## Verifying

- Windows: `signtool verify /pa forge_ide_*_x64-setup.exe`.
- macOS: `spctl -a -vvv -t install Forge.ML_*.dmg` should report `accepted` / `source=Notarized Developer ID`.

## Note on the `if:` gating

Each signing step is guarded by `env.<SECRET> != ''`, so the pipeline keeps building
unsigned installers until the secrets exist, then starts signing automatically — no other
workflow changes needed. Remove the note about unsigned installers from `site/index.html`
once signing is live.
