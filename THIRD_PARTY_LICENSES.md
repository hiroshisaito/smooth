# Third-Party Licenses and Notices

This file lists third-party components that are linked into, or required to
build, the current smooth macOS and Windows plugin binaries.

The smooth project itself is distributed under the Apache License, Version 2.0.
See `LICENSE` for the project license text, upstream LoiLo notice, and trademark
notice. Include both `LICENSE` and this file with source and binary
redistributions.

This inventory was verified on 2026-05-05 from:

- `rust/smooth_core/Cargo.lock`
- `cargo tree --locked --target x86_64-pc-windows-msvc --edges normal,no-proc-macro`
- `cargo tree --locked --target aarch64-apple-darwin --edges normal,no-proc-macro`
- `cargo tree --locked --target x86_64-apple-darwin --edges normal,no-proc-macro`
- `cargo metadata --locked --offline --filter-platform x86_64-pc-windows-msvc`
- `cargo metadata --locked --offline --filter-platform aarch64-apple-darwin`
- `cargo metadata --locked --offline --filter-platform x86_64-apple-darwin`

## Apache-2.0 Compatibility Summary

The runtime Rust dependencies linked into the plugin binaries are licensed under
MIT, Apache-2.0, or MIT/Apache-2.0 dual-license expressions.

The source/build-time Rust dependencies used by proc macros and build scripts
are also permissively licensed. `unicode-ident` additionally carries the
Unicode License v3 for Unicode data. No GPL, LGPL, AGPL, or MPL dependency was
found in the current production/build dependency set.

Based on this dependency set, smooth can continue to be distributed as an
Apache-2.0 project, provided the third-party copyright and license notices are
preserved.

## Runtime Rust Dependencies

These crates are linked into the production plugin binaries for the listed
targets. License expressions are taken from Cargo package metadata.

### Common to macOS and Windows

| Package | Version | License | Notice holder / project |
| --- | --- | --- | --- |
| crossbeam-deque | 0.8.6 | MIT OR Apache-2.0 | crossbeam-rs project |
| crossbeam-epoch | 0.9.18 | MIT OR Apache-2.0 | crossbeam-rs project |
| crossbeam-utils | 0.8.21 | MIT OR Apache-2.0 | crossbeam-rs project |
| either | 1.15.0 | MIT OR Apache-2.0 | bluss |
| rayon | 1.12.0 | MIT OR Apache-2.0 | rayon-rs project |
| rayon-core | 1.13.0 | MIT OR Apache-2.0 | rayon-rs project |

## SDKs, Toolchains, Test Dependencies, and Local Tools

Adobe After Effects SDK, Apple Xcode/macOS SDK, Microsoft Visual Studio, and
Microsoft Windows SDK are SDK or toolchain dependencies governed by their
respective vendor terms. They are expected to exist locally under
`references/` or vendor install locations for builds, but normal smooth source
and plugin binary distributions must not redistribute those SDKs.

Python packages used only by tests or fixture generation, such as Pillow, NumPy,
and OpenEXR, are not part of the smooth plugin binaries. If those packages are
redistributed separately, include their own license notices.

The 7-Zip-Zstandard files that may exist under an ignored local `references/`
directory are extraction tools only and are not part of normal smooth source or
plugin binary distributions. If redistributed separately, include the license
notices supplied with those files.

If Cargo dependencies are vendored or if a new platform target is added,
regenerate this file from the vendored source or the target-specific Cargo
metadata before release.

## MIT Permission Notice

For packages listed above whose license expression includes MIT, the applicable
copyright holders are the notice holders, package authors, or copyright notices
included in the corresponding package source. Preserve those notices together
with the following MIT permission notice:

```
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Apache License Text

For packages listed above whose license expression includes Apache-2.0, see the
Apache License, Version 2.0 text in `LICENSE`.

## Unicode License v3 Text

The `unicode-ident` crate includes Unicode data under the Unicode License v3:

```
UNICODE LICENSE V3

COPYRIGHT AND PERMISSION NOTICE

Copyright © 1991-2023 Unicode, Inc.

NOTICE TO USER: Carefully read the following legal agreement. BY
DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR
SOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE
TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT
DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.

Permission is hereby granted, free of charge, to any person obtaining a
copy of data files and any associated documentation (the "Data Files") or
software and any associated documentation (the "Software") to deal in the
Data Files or Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, and/or sell
copies of the Data Files or Software, and to permit persons to whom the
Data Files or Software are furnished to do so, provided that either (a)
this copyright and permission notice appear with all copies of the Data
Files or Software, or (b) this copyright and permission notice appear in
associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
THIRD PARTY RIGHTS.

IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE
BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES,
OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,
ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA
FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder shall
not be used in advertising or otherwise to promote the sale, use or other
dealings in these Data Files or Software without prior written
authorization of the copyright holder.
```
