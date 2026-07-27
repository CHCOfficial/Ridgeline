# macOS packaging

`scripts/package_macos.sh` builds the optimized arm64 binary, assembles a standard application
bundle in a clean staging area, installs the selected icon family, validates `Info.plist`, applies
and verifies a local ad-hoc signature, and creates a metadata-clean Finder ZIP.

```sh
./scripts/package_macos.sh classic
```

Available icon arguments are `classic`, `party`, and `contour`. The finished artifacts are written
to `dist/macos/`. Prefer the ZIP when moving the application elsewhere; it is generated from the
clean signed staging bundle and does not carry source-folder Finder metadata.

The local signature is sufficient for development and direct use on the build Mac. Public internet
distribution should replace it with a Developer ID signature and Apple notarization.
