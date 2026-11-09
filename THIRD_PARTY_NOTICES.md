# Third-party notices

The MIT OR Apache-2.0 license for Noir applies to Noir's original code. Third-party code and font assets retain their own copyright notices and license terms.

## Embedded fonts

Fonts are supplied by `epaint_default_fonts` 0.37.0. Noir's layout and CPU rasterizer use its unmodified `Ubuntu-Light.ttf`; the native shell uses egui's default font collection. Font data is embedded in the application, rather than installed as a system font.

| Font or asset | Copyright and attribution | License text |
| --- | --- | --- |
| Ubuntu Light | Copyright 2011 Canonical Ltd. Licensed under the Ubuntu Font Licence 1.0. | [Ubuntu Font Licence 1.0](licenses/fonts/UFL.txt) |
| Hack Regular | Copyright 2018 Source Foundry Authors. Includes Bitstream Vera Sans Mono, Copyright 2003 Bitstream Inc.; DejaVu contributions were committed to the public domain. | [Hack MIT and Bitstream Vera notices and licenses](licenses/fonts/Hack-Regular.txt) |
| egui-icons.ttf | A subset of emoji-icon-font by John Slegers, Copyright (c) 2014 John Slegers. The subset is supplied by egui. | [MIT license](licenses/fonts/emoji-icon-font-mit-license.txt) and [upstream subset description](licenses/fonts/egui-icons.txt) |

`epaint_default_fonts` also provides optional monochrome emoji assets: Noto Emoji, Copyright 2013 Google Inc., under the [SIL Open Font License 1.1](licenses/fonts/OFL.txt), and the full emoji-icon-font under the MIT license linked above. The current desktop feature graph does not enable `monochrome_emoji_fonts`; their notices are retained for future feature changes. See [font metadata](licenses/fonts/FONT-METADATA.txt) for copyright/license strings and SHA-256 values read directly from the font files in the installed crate.

The font licenses do not relicense Noir or documents rendered using the fonts. No rights to the Ubuntu, Android, Windows, GitHub, Apple or other names or trademarks are granted by this notice.

## Bundled local inference

Noir 0.5 distributions include a CPU inference runtime and model under `resources/ai/`. Their terms remain independent from Noir's original-code license:

| Resource | Pinned source | License and accompanying notice |
| --- | --- | --- |
| Qwen2.5-1.5B-Instruct Q4_K_M weights | Official `Qwen/Qwen2.5-1.5B-Instruct-GGUF`, revision `91cad51170dc346986eccefdc2dd33a9da36ead9` | Apache-2.0; `resources/ai/LICENSE-QWEN.txt` |
| llama.cpp CPU runtime | Official `ggml-org/llama.cpp` release `b11960`, selected by stable `v0.8.0` | MIT; `resources/ai/LICENSE-LLAMA.txt` |
| LLVM OpenMP supplied in the Windows CPU archive | Unmodified `libomp.dll` and notices in the official archive | `resources/ai/runtime/LICENSE-LLVM-OpenMP` |

Sources, revisions and SHA-256 values are fixed in `tools/ai/manifest.json`. `tools/ai/setup.ps1` verifies the archive/model and records extracted-resource hashes. The smaller model evaluated during development is not included in the final bundle. Preserve all accompanying license/notice texts on redistribution; they grant no trademark rights or model-accuracy guarantees.

The runtime's libraries retain the notices supplied in its official CPU archive. New GPU libraries, backends and models require their own redistribution review. User-configured services and custom weights are not relicensed by Noir. Provenance and operation are documented in [local inference](docs/ai/runtime.md).

## Rust dependencies

The [licenses directory](licenses/README.md) contains an inventory generated from Cargo metadata and copies of license/notice files from the installed crates, plus upstream license files where the crate archive omits them. Each dependency retains the license expression declared in its manifest. The inventory includes resolved packages for its target and may include build, test and optional dependencies; it is not a statement that every listed package is linked into every executable.

The initial inventory was generated for `x86_64-pc-windows-msvc`; `scripts/package.ps1` regenerates an inventory for the packaging host. `Cargo.lock` records the resolved versions. Neither license metadata nor collected files constitute a completed legal or security audit; source-code notices, embedded assets and any newly bundled platform libraries must also be reviewed before public distribution.

Full copied texts are under `licenses/dependencies/<crate>-<version>/`. Font-specific terms above remain applicable even when the crate's Rust code offers MIT OR Apache-2.0. The source URLs for supplemental notices are recorded in [upstream sources](licenses/upstream/SOURCES.md). For the egui family, the package's recorded Git commit was unavailable from the upstream raw endpoint at collection time, so the included Rust-code MIT/Apache texts were retrieved from the official repository's current `main` branch; the font texts came directly from the installed versioned crate.

## Static image decoder

The resvg/usvg/tiny-skia libraries are used solely for the bounded static SVG image subset in the broker. Default features are disabled and external image/file/font resolution is not provided. Their licenses and those of GIF/WebP codecs are included in the generated dependency inventory. They do not replace Noir’s HTML, CSS, layout or JavaScript engines.

The RegExp subset uses the Rust regex library for linear matching and fancy-regex 0.19.2 for a bounded advanced-pattern subset. These are pattern-search libraries, not embedded JavaScript engines; the language adapter, state, limits and VM remain Noir code. Dependency licenses are included in the inventory.
