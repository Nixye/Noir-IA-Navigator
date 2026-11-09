# Collected third-party license information

The parent [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md) describes the embedded fonts and the scope of these notices.

- `fonts/` contains the license texts distributed in `epaint_default_fonts` 0.37.0 and metadata extracted from the TTF files. It does not redistribute additional font binaries.
- `dependencies/` contains top-level LICENSE, LICENCE, COPYING, NOTICE and UNLICENSE files found in the installed crate archives, plus declared license files and supplemental upstream texts.
- `dependency-inventory-<target>.csv` records package names, versions, declared license expressions, repositories and the license files present in the collection. It includes resolved metadata packages for that target, including possible build, test or disabled optional packages.
- `upstream/` preserves supplemental license texts and their source URLs. The source record distinguishes an exact recorded commit from a repository branch fallback.

Regenerate metadata and copy locally available crate notices with:

```powershell
./scripts/collect-licenses.ps1
./scripts/collect-licenses.ps1 -Target x86_64-pc-windows-msvc
```

The default target is the Rust host returned by `rustc -vV`. Cargo may download the target's dependency metadata and sources. The script never removes older notices; versioned directories make those visible for review. On a dependency update, inspect new/removed packages, verify supplemental notices and refresh the font metadata when the font package changes. Empty or ambiguous manifest licenses and missing source notices require review rather than an inferred license.

The collection is an engineering aid, not a legal audit or an assertion that optional packages are linked. It does not cover additional binaries or system libraries bundled outside Cargo. Keep the directory and `THIRD_PARTY_NOTICES.md` with a redistributed Noir package.
