# RIDGELINE macOS icons

| Classic (default) | PARTY | Contour |
| --- | --- | --- |
| ![Classic](classic.png) | ![PARTY](party.png) | ![Contour](contour.png) |

The three original icon directions are included as 1024 px PNG artwork and complete `.icns`
families. **Classic** is the default application icon.

To select another icon while packaging:

```sh
./scripts/package_macos.sh party
./scripts/package_macos.sh contour
```

Run `./scripts/package_macos.sh classic` to restore the default. Each package also copies all three
`.icns` files into `dist/macos/Icon Choices`.
