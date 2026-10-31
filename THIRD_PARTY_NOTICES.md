# Third-party notices

These notices accompany Prometeu Search 0.1.0 and its bundled native binaries.
They record the dependencies resolved in the engine's standalone `Cargo.lock`,
including development, build-time, and platform-specific packages. Inclusion here does not mean that
every package is linked into every binary. Where a dependency offers alternative
licenses, the selected license below is the option used for this distribution.
Additional Unicode and other accompanying notices are retained separately.

The license texts are reproduced from the exact downloaded crate versions,
except where an upstream source URL identifies a license file omitted from its
crate archive. Identical texts are collected once and linked from each package.
The source archives linked below are available without charge.

## Source availability

Dependencies are used without source modifications. In particular,
`option-ext` 0.2.0 is distributed under MPL-2.0. Its complete corresponding
source is available in the
[exact crate source archive](https://crates.io/api/v1/crates/option-ext/0.2.0/download)
and [online source listing](https://docs.rs/crate/option-ext/0.2.0/source/).
The full MPL-2.0 text appears in the package's linked notice below.
The MPL applies to that component, not to the independently written Prometeu
Search source files. Keep this notice and source link with binary distributions.

## Bundled SQLite

The `bundled` feature of `libsqlite3-sys` 0.38.2 compiles SQLite 3.53.2 into the
native binary. SQLite is in the public domain. Its source dedication is retained
below; the Rust bindings have their own MIT license, listed separately.
SQLCipher and the WebAssembly SQLite implementation are not compiled into the
native artifacts. See [SQLite copyright](https://www.sqlite.org/copyright.html).

```text
The author disclaims copyright to this source code. In place of
a legal notice, here is a blessing:

    May you do good and not evil.
    May you find forgiveness for yourself and forgive others.
    May you share freely, never taking more than you give.
```

## Scope of the native artifacts

The binary matrix covers macOS arm64/x86_64 and Linux arm64/x86_64 GNU.
The following lockfile packages are not used by those artifacts:

- `rsqlite-vfs` 0.1.1: Only used by the WebAssembly SQLite backend; no WebAssembly target is distributed.
- `winapi-i686-pc-windows-gnu` 0.4.0: Windows GNU import library; no native Windows target is distributed.
- `winapi-x86_64-pc-windows-gnu` 0.4.0: Windows GNU import library; no native Windows target is distributed.

Reassess these exclusions before distributing other targets. Other unused
platform and development dependencies are included below as a conservative
superset because their license notices are available in the crate archives.

## Dependency inventory

### aho-corasick 1.1.5

Declared license: `Unlicense OR MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/aho-corasick/1.1.5) · [Source archive](https://crates.io/api/v1/crates/aho-corasick/1.1.5/download)

- `COPYING`: [Notice 1](#notice-1)
- `LICENSE-MIT`: [Notice 2](#notice-2)

### anstream 1.0.0

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/anstream/1.0.0) · [Source archive](https://crates.io/api/v1/crates/anstream/1.0.0/download)

- `LICENSE-MIT`: [Notice 3](#notice-3)

### anstyle 1.0.14

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/anstyle/1.0.14) · [Source archive](https://crates.io/api/v1/crates/anstyle/1.0.14/download)

- `LICENSE-MIT`: [Notice 3](#notice-3)

### anstyle-parse 1.0.0

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/anstyle-parse/1.0.0) · [Source archive](https://crates.io/api/v1/crates/anstyle-parse/1.0.0/download)

- `LICENSE-MIT`: [Notice 3](#notice-3)

### anstyle-query 1.1.5

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/anstyle-query/1.1.5) · [Source archive](https://crates.io/api/v1/crates/anstyle-query/1.1.5/download)

- `LICENSE-MIT`: [Notice 3](#notice-3)

### anstyle-wincon 3.0.11

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/anstyle-wincon/3.0.11) · [Source archive](https://crates.io/api/v1/crates/anstyle-wincon/3.0.11/download)

- `LICENSE-MIT`: [Notice 3](#notice-3)

### anyhow 1.0.104

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/anyhow/1.0.104) · [Source archive](https://crates.io/api/v1/crates/anyhow/1.0.104/download)

- `LICENSE-MIT`: [Notice 4](#notice-4)

### arrayvec 0.7.8

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/arrayvec/0.7.8) · [Source archive](https://crates.io/api/v1/crates/arrayvec/0.7.8/download)

- `LICENSE-MIT`: [Notice 5](#notice-5)

### bitflags 2.13.2

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/bitflags/2.13.2) · [Source archive](https://crates.io/api/v1/crates/bitflags/2.13.2/download)

- `LICENSE-MIT`: [Notice 6](#notice-6)

### blake3 1.8.7

Declared license: `CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception`. Selected: `CC0-1.0`.

[Crate and source](https://crates.io/crates/blake3/1.8.7) · [Source archive](https://crates.io/api/v1/crates/blake3/1.8.7/download)

- `LICENSE_CC0`: [Notice 7](#notice-7)

### bstr 1.13.1

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/bstr/1.13.1) · [Source archive](https://crates.io/api/v1/crates/bstr/1.13.1/download)

- `COPYING`: [Notice 8](#notice-8)
- `LICENSE-MIT`: [Notice 9](#notice-9)

### bumpalo 3.20.3

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/bumpalo/3.20.3) · [Source archive](https://crates.io/api/v1/crates/bumpalo/3.20.3/download)

- `LICENSE-MIT`: [Notice 10](#notice-10)

### cc 1.10.0

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/cc/1.10.0) · [Source archive](https://crates.io/api/v1/crates/cc/1.10.0/download)

- `LICENSE-MIT`: [Notice 11](#notice-11)

### cfg-if 1.0.5

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/cfg-if/1.0.5) · [Source archive](https://crates.io/api/v1/crates/cfg-if/1.0.5/download)

- `LICENSE-MIT`: [Notice 11](#notice-11)

### clap 4.6.7

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/clap/4.6.7) · [Source archive](https://crates.io/api/v1/crates/clap/4.6.7/download)

- `LICENSE-MIT`: [Notice 3](#notice-3)

### clap_builder 4.6.7

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/clap_builder/4.6.7) · [Source archive](https://crates.io/api/v1/crates/clap_builder/4.6.7/download)

- `LICENSE-MIT`: [Notice 3](#notice-3)

### clap_derive 4.6.7

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/clap_derive/4.6.7) · [Source archive](https://crates.io/api/v1/crates/clap_derive/4.6.7/download)

- `LICENSE-MIT`: [Notice 3](#notice-3)

### clap_lex 1.1.1

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/clap_lex/1.1.1) · [Source archive](https://crates.io/api/v1/crates/clap_lex/1.1.1/download)

- `LICENSE-MIT`: [Notice 3](#notice-3)

### colorchoice 1.0.5

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/colorchoice/1.0.5) · [Source archive](https://crates.io/api/v1/crates/colorchoice/1.0.5/download)

- `LICENSE-MIT`: [Notice 3](#notice-3)

### constant_time_eq 0.4.2

Declared license: `CC0-1.0 OR MIT-0 OR Apache-2.0`. Selected: `MIT-0`.

[Crate and source](https://crates.io/crates/constant_time_eq/0.4.2) · [Source archive](https://crates.io/api/v1/crates/constant_time_eq/0.4.2/download)

- `LICENSE-MIT0`: [Notice 12](#notice-12)

### cpufeatures 0.3.1

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/cpufeatures/0.3.1) · [Source archive](https://crates.io/api/v1/crates/cpufeatures/0.3.1/download)

- `LICENSE-MIT`: [Notice 13](#notice-13)

### crossbeam-deque 0.8.8

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/crossbeam-deque/0.8.8) · [Source archive](https://crates.io/api/v1/crates/crossbeam-deque/0.8.8/download)

- `LICENSE-MIT`: [Notice 14](#notice-14)

### crossbeam-epoch 0.9.21

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/crossbeam-epoch/0.9.21) · [Source archive](https://crates.io/api/v1/crates/crossbeam-epoch/0.9.21/download)

- `LICENSE-MIT`: [Notice 14](#notice-14)

### crossbeam-utils 0.8.23

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/crossbeam-utils/0.8.23) · [Source archive](https://crates.io/api/v1/crates/crossbeam-utils/0.8.23/download)

- `LICENSE-MIT`: [Notice 14](#notice-14)

### dirs 6.0.0

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/dirs/6.0.0) · [Source archive](https://crates.io/api/v1/crates/dirs/6.0.0/download)

- `LICENSE-MIT`: [Notice 15](#notice-15)

### dirs-sys 0.5.0

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/dirs-sys/0.5.0) · [Source archive](https://crates.io/api/v1/crates/dirs-sys/0.5.0/download)

- `LICENSE-MIT`: [Notice 15](#notice-15)

### equivalent 1.0.3

Declared license: `Apache-2.0 OR MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/equivalent/1.0.3) · [Source archive](https://crates.io/api/v1/crates/equivalent/1.0.3/download)

- `LICENSE-MIT`: [Notice 16](#notice-16)

### errno 0.3.14

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/errno/0.3.14) · [Source archive](https://crates.io/api/v1/crates/errno/0.3.14/download)

- `LICENSE-MIT`: [Notice 17](#notice-17)

### fallible-iterator 0.3.0

Declared license: `MIT/Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/fallible-iterator/0.3.0) · [Source archive](https://crates.io/api/v1/crates/fallible-iterator/0.3.0/download)

- `LICENSE-MIT`: [Notice 18](#notice-18)

### fallible-streaming-iterator 0.1.9

Declared license: `MIT/Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/fallible-streaming-iterator/0.1.9) · [Source archive](https://crates.io/api/v1/crates/fallible-streaming-iterator/0.1.9/download)

- `LICENSE-MIT`: [Notice 19](#notice-19)

### fastrand 2.5.0

Declared license: `Apache-2.0 OR MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/fastrand/2.5.0) · [Source archive](https://crates.io/api/v1/crates/fastrand/2.5.0/download)

- `LICENSE-MIT`: [Notice 4](#notice-4)

### find-msvc-tools 0.1.15

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/find-msvc-tools/0.1.15) · [Source archive](https://crates.io/api/v1/crates/find-msvc-tools/0.1.15/download)

- `LICENSE-MIT`: [Notice 11](#notice-11)

### foldhash 0.2.0

Declared license: `Zlib`. Selected: `Zlib`.

[Crate and source](https://crates.io/crates/foldhash/0.2.0) · [Source archive](https://crates.io/api/v1/crates/foldhash/0.2.0/download)

- `LICENSE`: [Notice 20](#notice-20)

### fs2 0.4.3

Declared license: `MIT/Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/fs2/0.4.3) · [Source archive](https://crates.io/api/v1/crates/fs2/0.4.3/download)

- `LICENSE-MIT`: [Notice 21](#notice-21)

### getrandom 0.4.3

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/getrandom/0.4.3) · [Source archive](https://crates.io/api/v1/crates/getrandom/0.4.3/download)

- `LICENSE-MIT`: [Notice 22](#notice-22)

### globset 0.4.21

Declared license: `Unlicense OR MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/globset/0.4.21) · [Source archive](https://crates.io/api/v1/crates/globset/0.4.21/download)

- `COPYING`: [Notice 1](#notice-1)
- `LICENSE-MIT`: [Notice 2](#notice-2)

### hashbrown 0.16.1

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/hashbrown/0.16.1) · [Source archive](https://crates.io/api/v1/crates/hashbrown/0.16.1/download)

- `LICENSE-MIT`: [Notice 23](#notice-23)

### hashbrown 0.17.1

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/hashbrown/0.17.1) · [Source archive](https://crates.io/api/v1/crates/hashbrown/0.17.1/download)

- `LICENSE-MIT`: [Notice 23](#notice-23)

### hashlink 0.12.2

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/hashlink/0.12.2) · [Source archive](https://crates.io/api/v1/crates/hashlink/0.12.2/download)

- `LICENSE-MIT`: [Notice 24](#notice-24)

### heck 0.5.0

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/heck/0.5.0) · [Source archive](https://crates.io/api/v1/crates/heck/0.5.0/download)

- `LICENSE-MIT`: [Notice 21](#notice-21)

### ignore 0.4.34

Declared license: `Unlicense OR MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/ignore/0.4.34) · [Source archive](https://crates.io/api/v1/crates/ignore/0.4.34/download)

- `COPYING`: [Notice 1](#notice-1)
- `LICENSE-MIT`: [Notice 2](#notice-2)

### indexmap 2.14.2

Declared license: `Apache-2.0 OR MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/indexmap/2.14.2) · [Source archive](https://crates.io/api/v1/crates/indexmap/2.14.2/download)

- `LICENSE-MIT`: [Notice 25](#notice-25)

### inotify 0.11.5

Declared license: `ISC`. Selected: `ISC`.

[Crate and source](https://crates.io/crates/inotify/0.11.5) · [Source archive](https://crates.io/api/v1/crates/inotify/0.11.5/download)

- `LICENSE`: [Notice 26](#notice-26)

### inotify-sys 0.1.8

Declared license: `ISC`. Selected: `ISC`.

[Crate and source](https://crates.io/crates/inotify-sys/0.1.8) · [Source archive](https://crates.io/api/v1/crates/inotify-sys/0.1.8/download)

- `LICENSE`: [Notice 26](#notice-26)

### is_terminal_polyfill 1.70.2

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/is_terminal_polyfill/1.70.2) · [Source archive](https://crates.io/api/v1/crates/is_terminal_polyfill/1.70.2/download)

- `LICENSE-MIT`: [Notice 3](#notice-3)

### itoa 1.0.18

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/itoa/1.0.18) · [Source archive](https://crates.io/api/v1/crates/itoa/1.0.18/download)

- `LICENSE-MIT`: [Notice 4](#notice-4)

### js-sys 0.3.107

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/js-sys/0.3.107) · [Source archive](https://crates.io/api/v1/crates/js-sys/0.3.107/download)

- `LICENSE-MIT`: [Notice 11](#notice-11)

### kqueue 1.2.1

Declared license: `MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/kqueue/1.2.1) · [Source archive](https://crates.io/api/v1/crates/kqueue/1.2.1/download)

- `LICENSE`: [Notice 27](#notice-27)

### kqueue-sys 1.1.2

Declared license: `MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/kqueue-sys/1.1.2) · [Source archive](https://crates.io/api/v1/crates/kqueue-sys/1.1.2/download)

- `LICENSE`: [Notice 27](#notice-27)

### libc 0.2.191

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/libc/0.2.191) · [Source archive](https://crates.io/api/v1/crates/libc/0.2.191/download)

- `LICENSE-MIT`: [Notice 28](#notice-28)

### libredox 0.1.25

Declared license: `MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/libredox/0.1.25) · [Source archive](https://crates.io/api/v1/crates/libredox/0.1.25/download)

- `LICENSE`: [Notice 29](#notice-29)

### libsqlite3-sys 0.38.2

Declared license: `MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/libsqlite3-sys/0.38.2) · [Source archive](https://crates.io/api/v1/crates/libsqlite3-sys/0.38.2/download)

- `LICENSE`: [Notice 30](#notice-30)

### linux-raw-sys 0.12.1

Declared license: `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/linux-raw-sys/0.12.1) · [Source archive](https://crates.io/api/v1/crates/linux-raw-sys/0.12.1/download)

- `LICENSE-MIT`: [Notice 4](#notice-4)

### log 0.4.35

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/log/0.4.35) · [Source archive](https://crates.io/api/v1/crates/log/0.4.35/download)

- `LICENSE-MIT`: [Notice 6](#notice-6)

### memchr 2.8.3

Declared license: `Unlicense OR MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/memchr/2.8.3) · [Source archive](https://crates.io/api/v1/crates/memchr/2.8.3/download)

- `COPYING`: [Notice 1](#notice-1)
- `LICENSE-MIT`: [Notice 2](#notice-2)

### mio 1.2.4

Declared license: `MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/mio/1.2.4) · [Source archive](https://crates.io/api/v1/crates/mio/1.2.4/download)

- `LICENSE`: [Notice 31](#notice-31)

### notify 8.2.1

Declared license: `CC0-1.0`. Selected: `CC0-1.0`.

[Crate and source](https://crates.io/crates/notify/8.2.1) · [Source archive](https://crates.io/api/v1/crates/notify/8.2.1/download)

- `LICENSE-CC0`: [Notice 32](#notice-32)

### notify-types 2.2.0

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/notify-types/2.2.0) · [Source archive](https://crates.io/api/v1/crates/notify-types/2.2.0/download)

- `LICENSE-MIT`: [Notice 33](#notice-33)

### objc2-core-foundation 0.3.2

Declared license: `Zlib OR Apache-2.0 OR MIT`. Selected: `Apache-2.0`.

[Crate and source](https://crates.io/crates/objc2-core-foundation/0.3.2) · [Source archive](https://crates.io/api/v1/crates/objc2-core-foundation/0.3.2/download)

- `LICENSE.md (upstream packaged commit)`: [Notice 34](#notice-34) ([upstream source](https://github.com/madsmtm/objc2/blob/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md))
- `Apache License, Version 2.0 (selected terms)`: [Notice 35](#notice-35) ([upstream source](https://www.apache.org/licenses/LICENSE-2.0))

### objc2-core-services 0.3.2

Declared license: `Zlib OR Apache-2.0 OR MIT`. Selected: `Apache-2.0`.

[Crate and source](https://crates.io/crates/objc2-core-services/0.3.2) · [Source archive](https://crates.io/api/v1/crates/objc2-core-services/0.3.2/download)

- `LICENSE.md (upstream packaged commit)`: [Notice 34](#notice-34) ([upstream source](https://github.com/madsmtm/objc2/blob/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md))
- `Apache License, Version 2.0 (selected terms)`: [Notice 35](#notice-35) ([upstream source](https://www.apache.org/licenses/LICENSE-2.0))

### once_cell 1.21.4

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/once_cell/1.21.4) · [Source archive](https://crates.io/api/v1/crates/once_cell/1.21.4/download)

- `LICENSE-MIT`: [Notice 4](#notice-4)

### once_cell_polyfill 1.70.2

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/once_cell_polyfill/1.70.2) · [Source archive](https://crates.io/api/v1/crates/once_cell_polyfill/1.70.2/download)

- `LICENSE-MIT`: [Notice 3](#notice-3)

### option-ext 0.2.0

Declared license: `MPL-2.0`. Selected: `MPL-2.0`.

[Crate and source](https://crates.io/crates/option-ext/0.2.0) · [Source archive](https://crates.io/api/v1/crates/option-ext/0.2.0/download)

- `LICENSE.txt`: [Notice 36](#notice-36)

### pkg-config 0.3.34

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/pkg-config/0.3.34) · [Source archive](https://crates.io/api/v1/crates/pkg-config/0.3.34/download)

- `LICENSE-MIT`: [Notice 11](#notice-11)

### proc-macro2 1.0.107

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/proc-macro2/1.0.107) · [Source archive](https://crates.io/api/v1/crates/proc-macro2/1.0.107/download)

- `LICENSE-MIT`: [Notice 4](#notice-4)

### quote 1.0.47

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/quote/1.0.47) · [Source archive](https://crates.io/api/v1/crates/quote/1.0.47/download)

- `LICENSE-MIT`: [Notice 4](#notice-4)

### r-efi 6.0.0

Declared license: `MIT OR Apache-2.0 OR LGPL-2.1-or-later`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/r-efi/6.0.0) · [Source archive](https://crates.io/api/v1/crates/r-efi/6.0.0/download)

- `AUTHORS`: [Notice 37](#notice-37)

### redox_users 0.5.3

Declared license: `MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/redox_users/0.5.3) · [Source archive](https://crates.io/api/v1/crates/redox_users/0.5.3/download)

- `LICENSE`: [Notice 38](#notice-38)

### regex 1.13.1

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/regex/1.13.1) · [Source archive](https://crates.io/api/v1/crates/regex/1.13.1/download)

- `LICENSE-MIT`: [Notice 6](#notice-6)

### regex-automata 0.4.18

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/regex-automata/0.4.18) · [Source archive](https://crates.io/api/v1/crates/regex-automata/0.4.18/download)

- `LICENSE-MIT`: [Notice 6](#notice-6)

### regex-syntax 0.8.11

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/regex-syntax/0.8.11) · [Source archive](https://crates.io/api/v1/crates/regex-syntax/0.8.11/download)

- `LICENSE-MIT`: [Notice 6](#notice-6)
- `src/unicode_tables/LICENSE-UNICODE`: [Notice 39](#notice-39)

### rusqlite 0.40.2

Declared license: `MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/rusqlite/0.40.2) · [Source archive](https://crates.io/api/v1/crates/rusqlite/0.40.2/download)

- `LICENSE`: [Notice 30](#notice-30)

### rustix 1.1.6

Declared license: `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/rustix/1.1.6) · [Source archive](https://crates.io/api/v1/crates/rustix/1.1.6/download)

- `LICENSE-MIT`: [Notice 4](#notice-4)

### rustversion 1.0.23

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/rustversion/1.0.23) · [Source archive](https://crates.io/api/v1/crates/rustversion/1.0.23/download)

- `LICENSE-MIT`: [Notice 4](#notice-4)

### same-file 1.0.6

Declared license: `Unlicense/MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/same-file/1.0.6) · [Source archive](https://crates.io/api/v1/crates/same-file/1.0.6/download)

- `COPYING`: [Notice 1](#notice-1)
- `LICENSE-MIT`: [Notice 40](#notice-40)

### serde 1.0.229

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/serde/1.0.229) · [Source archive](https://crates.io/api/v1/crates/serde/1.0.229/download)

- `LICENSE-MIT`: [Notice 4](#notice-4)

### serde_core 1.0.229

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/serde_core/1.0.229) · [Source archive](https://crates.io/api/v1/crates/serde_core/1.0.229/download)

- `LICENSE-MIT`: [Notice 4](#notice-4)

### serde_derive 1.0.229

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/serde_derive/1.0.229) · [Source archive](https://crates.io/api/v1/crates/serde_derive/1.0.229/download)

- `LICENSE-MIT`: [Notice 4](#notice-4)

### serde_json 1.0.154

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/serde_json/1.0.154) · [Source archive](https://crates.io/api/v1/crates/serde_json/1.0.154/download)

- `LICENSE-MIT`: [Notice 4](#notice-4)

### shlex 2.0.1

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/shlex/2.0.1) · [Source archive](https://crates.io/api/v1/crates/shlex/2.0.1/download)

- `LICENSE-MIT`: [Notice 41](#notice-41)

### smallvec 1.16.4

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/smallvec/1.16.4) · [Source archive](https://crates.io/api/v1/crates/smallvec/1.16.4/download)

- `LICENSE-MIT`: [Notice 42](#notice-42)

### sqlite-wasm-rs 0.5.5

Declared license: `MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/sqlite-wasm-rs/0.5.5) · [Source archive](https://crates.io/api/v1/crates/sqlite-wasm-rs/0.5.5/download)

- `LICENSE`: [Notice 43](#notice-43)

### streaming-iterator 0.1.9

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/streaming-iterator/0.1.9) · [Source archive](https://crates.io/api/v1/crates/streaming-iterator/0.1.9/download)

- `LICENSE-MIT`: [Notice 44](#notice-44)

### strsim 0.11.1

Declared license: `MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/strsim/0.11.1) · [Source archive](https://crates.io/api/v1/crates/strsim/0.11.1/download)

- `LICENSE`: [Notice 45](#notice-45)

### syn 3.0.9

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/syn/3.0.9) · [Source archive](https://crates.io/api/v1/crates/syn/3.0.9/download)

- `LICENSE-MIT`: [Notice 4](#notice-4)

### tempfile 3.27.0

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/tempfile/3.27.0) · [Source archive](https://crates.io/api/v1/crates/tempfile/3.27.0/download)

- `LICENSE-MIT`: [Notice 46](#notice-46)

### thiserror 2.0.21

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/thiserror/2.0.21) · [Source archive](https://crates.io/api/v1/crates/thiserror/2.0.21/download)

- `LICENSE-MIT`: [Notice 4](#notice-4)

### thiserror-impl 2.0.21

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/thiserror-impl/2.0.21) · [Source archive](https://crates.io/api/v1/crates/thiserror-impl/2.0.21/download)

- `LICENSE-MIT`: [Notice 4](#notice-4)

### tree-sitter 0.25.10

Declared license: `MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/tree-sitter/0.25.10) · [Source archive](https://crates.io/api/v1/crates/tree-sitter/0.25.10/download)

- `LICENSE (upstream packaged commit)`: [Notice 48](#notice-48) ([upstream source](https://github.com/tree-sitter/tree-sitter/blob/da6fe9beb4f7f67beb75914ca8e0d48ae48d6406/LICENSE))
- `src/unicode/LICENSE`: [Notice 47](#notice-47)

### tree-sitter-javascript 0.25.0

Declared license: `MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/tree-sitter-javascript/0.25.0) · [Source archive](https://crates.io/api/v1/crates/tree-sitter-javascript/0.25.0/download)

- `LICENSE`: [Notice 49](#notice-49)

### tree-sitter-language 0.1.7

Declared license: `MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/tree-sitter-language/0.1.7) · [Source archive](https://crates.io/api/v1/crates/tree-sitter-language/0.1.7/download)

- `LICENSE`: [Notice 50](#notice-50)

### tree-sitter-ruby 0.23.1

Declared license: `MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/tree-sitter-ruby/0.23.1) · [Source archive](https://crates.io/api/v1/crates/tree-sitter-ruby/0.23.1/download)

- `LICENSE (upstream packaged commit)`: [Notice 58](#notice-58) ([upstream source](https://github.com/tree-sitter/tree-sitter-ruby/blob/71bd32fb7607035768799732addba884a37a6210/LICENSE))

### tree-sitter-rust 0.24.2

Declared license: `MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/tree-sitter-rust/0.24.2) · [Source archive](https://crates.io/api/v1/crates/tree-sitter-rust/0.24.2/download)

- `LICENSE`: [Notice 51](#notice-51)

### tree-sitter-typescript 0.23.2

Declared license: `MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/tree-sitter-typescript/0.23.2) · [Source archive](https://crates.io/api/v1/crates/tree-sitter-typescript/0.23.2/download)

- `LICENSE (upstream packaged commit)`: [Notice 52](#notice-52) ([upstream source](https://github.com/tree-sitter/tree-sitter-typescript/blob/f975a621f4e7f532fe322e13c4f79495e0a7b2e7/LICENSE))

### unicode-ident 1.0.27

Declared license: `(MIT OR Apache-2.0) AND Unicode-3.0`. Selected: `MIT AND Unicode-3.0`.

[Crate and source](https://crates.io/crates/unicode-ident/1.0.27) · [Source archive](https://crates.io/api/v1/crates/unicode-ident/1.0.27/download)

- `LICENSE-MIT`: [Notice 4](#notice-4)
- `LICENSE-UNICODE`: [Notice 53](#notice-53)

### utf8parse 0.2.2

Declared license: `Apache-2.0 OR MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/utf8parse/0.2.2) · [Source archive](https://crates.io/api/v1/crates/utf8parse/0.2.2/download)

- `LICENSE-MIT`: [Notice 54](#notice-54)

### vcpkg 0.2.15

Declared license: `MIT/Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/vcpkg/0.2.15) · [Source archive](https://crates.io/api/v1/crates/vcpkg/0.2.15/download)

- `LICENSE-MIT`: [Notice 55](#notice-55)

### walkdir 2.5.0

Declared license: `Unlicense/MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/walkdir/2.5.0) · [Source archive](https://crates.io/api/v1/crates/walkdir/2.5.0/download)

- `COPYING`: [Notice 1](#notice-1)
- `LICENSE-MIT`: [Notice 2](#notice-2)

### wasi 0.11.1+wasi-snapshot-preview1

Declared license: `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/wasi/0.11.1+wasi-snapshot-preview1) · [Source archive](https://crates.io/api/v1/crates/wasi/0.11.1+wasi-snapshot-preview1/download)

- `LICENSE-MIT`: [Notice 4](#notice-4)

### wasm-bindgen 0.2.130

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/wasm-bindgen/0.2.130) · [Source archive](https://crates.io/api/v1/crates/wasm-bindgen/0.2.130/download)

- `LICENSE-MIT`: [Notice 11](#notice-11)

### wasm-bindgen-macro 0.2.130

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/wasm-bindgen-macro/0.2.130) · [Source archive](https://crates.io/api/v1/crates/wasm-bindgen-macro/0.2.130/download)

- `LICENSE-MIT`: [Notice 11](#notice-11)

### wasm-bindgen-macro-support 0.2.130

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/wasm-bindgen-macro-support/0.2.130) · [Source archive](https://crates.io/api/v1/crates/wasm-bindgen-macro-support/0.2.130/download)

- `LICENSE-MIT`: [Notice 11](#notice-11)

### wasm-bindgen-shared 0.2.130

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/wasm-bindgen-shared/0.2.130) · [Source archive](https://crates.io/api/v1/crates/wasm-bindgen-shared/0.2.130/download)

- `LICENSE-MIT`: [Notice 11](#notice-11)

### winapi 0.3.9

Declared license: `MIT/Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/winapi/0.3.9) · [Source archive](https://crates.io/api/v1/crates/winapi/0.3.9/download)

- `LICENSE-MIT`: [Notice 56](#notice-56)

### winapi-util 0.1.11

Declared license: `Unlicense OR MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/winapi-util/0.1.11) · [Source archive](https://crates.io/api/v1/crates/winapi-util/0.1.11/download)

- `COPYING`: [Notice 1](#notice-1)
- `LICENSE-MIT`: [Notice 40](#notice-40)

### windows-link 0.2.1

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows-link/0.2.1) · [Source archive](https://crates.io/api/v1/crates/windows-link/0.2.1/download)

- `license-mit`: [Notice 57](#notice-57)

### windows-sys 0.61.2

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows-sys/0.61.2) · [Source archive](https://crates.io/api/v1/crates/windows-sys/0.61.2/download)

- `license-mit`: [Notice 57](#notice-57)

### zmij 1.0.23

Declared license: `MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/zmij/1.0.23) · [Source archive](https://crates.io/api/v1/crates/zmij/1.0.23/download)

- `LICENSE-MIT`: [Notice 4](#notice-4)

### ambient-authority 0.0.2

Declared license: `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/ambient-authority/0.0.2) · [Source archive](https://crates.io/api/v1/crates/ambient-authority/0.0.2/download)

- `COPYRIGHT`: [Notice 59](#notice-59)
- `LICENSE-MIT`: [Notice 4](#notice-4)

### cap-fs-ext 4.0.3

Declared license: `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/cap-fs-ext/4.0.3) · [Source archive](https://crates.io/api/v1/crates/cap-fs-ext/4.0.3/download)

- `COPYRIGHT`: [Notice 60](#notice-60)
- `LICENSE-MIT`: [Notice 4](#notice-4)

### cap-primitives 4.0.3

Declared license: `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/cap-primitives/4.0.3) · [Source archive](https://crates.io/api/v1/crates/cap-primitives/4.0.3/download)

- `COPYRIGHT`: [Notice 61](#notice-61)
- `LICENSE-MIT`: [Notice 4](#notice-4)

### cap-std 4.0.3

Declared license: `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/cap-std/4.0.3) · [Source archive](https://crates.io/api/v1/crates/cap-std/4.0.3/download)

- `COPYRIGHT`: [Notice 62](#notice-62)
- `LICENSE-MIT`: [Notice 4](#notice-4)

### fs-set-times 0.20.3

Declared license: `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/fs-set-times/0.20.3) · [Source archive](https://crates.io/api/v1/crates/fs-set-times/0.20.3/download)

- `COPYRIGHT`: [Notice 63](#notice-63)
- `LICENSE-MIT`: [Notice 4](#notice-4)

### io-extras 0.19.0

Declared license: `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/io-extras/0.19.0) · [Source archive](https://crates.io/api/v1/crates/io-extras/0.19.0/download)

- `COPYRIGHT`: [Notice 64](#notice-64)
- `LICENSE-MIT`: [Notice 4](#notice-4)

### io-lifetimes 2.0.4

Declared license: `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/io-lifetimes/2.0.4) · [Source archive](https://crates.io/api/v1/crates/io-lifetimes/2.0.4/download)

- `COPYRIGHT`: [Notice 65](#notice-65)
- `LICENSE-MIT`: [Notice 4](#notice-4)

### io-lifetimes 3.0.1

Declared license: `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/io-lifetimes/3.0.1) · [Source archive](https://crates.io/api/v1/crates/io-lifetimes/3.0.1/download)

- `COPYRIGHT`: [Notice 65](#notice-65)
- `LICENSE-MIT`: [Notice 4](#notice-4)

### ipnet 2.12.2

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/ipnet/2.12.2) · [Source archive](https://crates.io/api/v1/crates/ipnet/2.12.2/download)

- `LICENSE-MIT`: [Notice 66](#notice-66)

### maybe-owned 0.3.4

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/maybe-owned/0.3.4) · [Source archive](https://crates.io/api/v1/crates/maybe-owned/0.3.4/download)

- `LICENSE-MIT`: [Notice 67](#notice-67)

### rustix-linux-procfs 0.1.1

Declared license: `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/rustix-linux-procfs/0.1.1) · [Source archive](https://crates.io/api/v1/crates/rustix-linux-procfs/0.1.1/download)

- `COPYRIGHT`: [Notice 68](#notice-68)
- `LICENSE-MIT`: [Notice 4](#notice-4)

### windows-sys 0.59.0

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows-sys/0.59.0) · [Source archive](https://crates.io/api/v1/crates/windows-sys/0.59.0/download)

- `license-mit`: [Notice 57](#notice-57)

### windows-sys 0.60.2

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows-sys/0.60.2) · [Source archive](https://crates.io/api/v1/crates/windows-sys/0.60.2/download)

- `license-mit`: [Notice 57](#notice-57)

### windows-targets 0.52.6

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows-targets/0.52.6) · [Source archive](https://crates.io/api/v1/crates/windows-targets/0.52.6/download)

- `license-mit`: [Notice 57](#notice-57)

### windows-targets 0.53.5

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows-targets/0.53.5) · [Source archive](https://crates.io/api/v1/crates/windows-targets/0.53.5/download)

- `license-mit`: [Notice 57](#notice-57)

### windows_aarch64_gnullvm 0.52.6

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows_aarch64_gnullvm/0.52.6) · [Source archive](https://crates.io/api/v1/crates/windows_aarch64_gnullvm/0.52.6/download)

- `license-mit`: [Notice 57](#notice-57)

### windows_aarch64_gnullvm 0.53.1

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows_aarch64_gnullvm/0.53.1) · [Source archive](https://crates.io/api/v1/crates/windows_aarch64_gnullvm/0.53.1/download)

- `license-mit`: [Notice 57](#notice-57)

### windows_aarch64_msvc 0.52.6

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows_aarch64_msvc/0.52.6) · [Source archive](https://crates.io/api/v1/crates/windows_aarch64_msvc/0.52.6/download)

- `license-mit`: [Notice 57](#notice-57)

### windows_aarch64_msvc 0.53.1

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows_aarch64_msvc/0.53.1) · [Source archive](https://crates.io/api/v1/crates/windows_aarch64_msvc/0.53.1/download)

- `license-mit`: [Notice 57](#notice-57)

### windows_i686_gnu 0.52.6

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows_i686_gnu/0.52.6) · [Source archive](https://crates.io/api/v1/crates/windows_i686_gnu/0.52.6/download)

- `license-mit`: [Notice 57](#notice-57)

### windows_i686_gnu 0.53.1

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows_i686_gnu/0.53.1) · [Source archive](https://crates.io/api/v1/crates/windows_i686_gnu/0.53.1/download)

- `license-mit`: [Notice 57](#notice-57)

### windows_i686_gnullvm 0.52.6

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows_i686_gnullvm/0.52.6) · [Source archive](https://crates.io/api/v1/crates/windows_i686_gnullvm/0.52.6/download)

- `license-mit`: [Notice 57](#notice-57)

### windows_i686_gnullvm 0.53.1

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows_i686_gnullvm/0.53.1) · [Source archive](https://crates.io/api/v1/crates/windows_i686_gnullvm/0.53.1/download)

- `license-mit`: [Notice 57](#notice-57)

### windows_i686_msvc 0.52.6

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows_i686_msvc/0.52.6) · [Source archive](https://crates.io/api/v1/crates/windows_i686_msvc/0.52.6/download)

- `license-mit`: [Notice 57](#notice-57)

### windows_i686_msvc 0.53.1

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows_i686_msvc/0.53.1) · [Source archive](https://crates.io/api/v1/crates/windows_i686_msvc/0.53.1/download)

- `license-mit`: [Notice 57](#notice-57)

### windows_x86_64_gnu 0.52.6

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows_x86_64_gnu/0.52.6) · [Source archive](https://crates.io/api/v1/crates/windows_x86_64_gnu/0.52.6/download)

- `license-mit`: [Notice 57](#notice-57)

### windows_x86_64_gnu 0.53.1

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows_x86_64_gnu/0.53.1) · [Source archive](https://crates.io/api/v1/crates/windows_x86_64_gnu/0.53.1/download)

- `license-mit`: [Notice 57](#notice-57)

### windows_x86_64_gnullvm 0.52.6

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows_x86_64_gnullvm/0.52.6) · [Source archive](https://crates.io/api/v1/crates/windows_x86_64_gnullvm/0.52.6/download)

- `license-mit`: [Notice 57](#notice-57)

### windows_x86_64_gnullvm 0.53.1

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows_x86_64_gnullvm/0.53.1) · [Source archive](https://crates.io/api/v1/crates/windows_x86_64_gnullvm/0.53.1/download)

- `license-mit`: [Notice 57](#notice-57)

### windows_x86_64_msvc 0.52.6

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows_x86_64_msvc/0.52.6) · [Source archive](https://crates.io/api/v1/crates/windows_x86_64_msvc/0.52.6/download)

- `license-mit`: [Notice 57](#notice-57)

### windows_x86_64_msvc 0.53.1

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/windows_x86_64_msvc/0.53.1) · [Source archive](https://crates.io/api/v1/crates/windows_x86_64_msvc/0.53.1/download)

- `license-mit`: [Notice 57](#notice-57)

### winx 0.36.4

Declared license: `Apache-2.0 WITH LLVM-exception`. Selected: `Apache-2.0 WITH LLVM-exception`.

[Crate and source](https://crates.io/crates/winx/0.36.4) · [Source archive](https://crates.io/api/v1/crates/winx/0.36.4/download)

- `LICENSE`: [Notice 69](#notice-69)

### pulldown-cmark 0.13.6

Declared license: `MIT`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/pulldown-cmark/0.13.6) · [Source archive](https://crates.io/api/v1/crates/pulldown-cmark/0.13.6/download)

- `LICENSE`: [Notice 70](#notice-70)

### unicase 2.10.0

Declared license: `MIT OR Apache-2.0`. Selected: `MIT`.

[Crate and source](https://crates.io/crates/unicase/2.10.0) · [Source archive](https://crates.io/api/v1/crates/unicase/2.10.0/download)

- `LICENSE-MIT`: [Notice 71](#notice-71)

## License and notice texts

### Notice 1

````text
This project is dual-licensed under the Unlicense and MIT licenses.

You may use this code under the terms of either license.
````

### Notice 2

````text
The MIT License (MIT)

Copyright (c) 2015 Andrew Gallant

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
````

### Notice 3

````text
Copyright (c) Individual contributors

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
````

### Notice 4

````text
Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
````

### Notice 5

````text
Copyright (c) Ulrik Sverdrup "bluss" 2015-2023

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
````

### Notice 6

````text
Copyright (c) 2014 The Rust Project Developers

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
````

### Notice 7

````text
Creative Commons Legal Code

CC0 1.0 Universal

    CREATIVE COMMONS CORPORATION IS NOT A LAW FIRM AND DOES NOT PROVIDE
    LEGAL SERVICES. DISTRIBUTION OF THIS DOCUMENT DOES NOT CREATE AN
    ATTORNEY-CLIENT RELATIONSHIP. CREATIVE COMMONS PROVIDES THIS
    INFORMATION ON AN "AS-IS" BASIS. CREATIVE COMMONS MAKES NO WARRANTIES
    REGARDING THE USE OF THIS DOCUMENT OR THE INFORMATION OR WORKS
    PROVIDED HEREUNDER, AND DISCLAIMS LIABILITY FOR DAMAGES RESULTING FROM
    THE USE OF THIS DOCUMENT OR THE INFORMATION OR WORKS PROVIDED
    HEREUNDER.

Statement of Purpose

The laws of most jurisdictions throughout the world automatically confer
exclusive Copyright and Related Rights (defined below) upon the creator
and subsequent owner(s) (each and all, an "owner") of an original work of
authorship and/or a database (each, a "Work").

Certain owners wish to permanently relinquish those rights to a Work for
the purpose of contributing to a commons of creative, cultural and
scientific works ("Commons") that the public can reliably and without fear
of later claims of infringement build upon, modify, incorporate in other
works, reuse and redistribute as freely as possible in any form whatsoever
and for any purposes, including without limitation commercial purposes.
These owners may contribute to the Commons to promote the ideal of a free
culture and the further production of creative, cultural and scientific
works, or to gain reputation or greater distribution for their Work in
part through the use and efforts of others.

For these and/or other purposes and motivations, and without any
expectation of additional consideration or compensation, the person
associating CC0 with a Work (the "Affirmer"), to the extent that he or she
is an owner of Copyright and Related Rights in the Work, voluntarily
elects to apply CC0 to the Work and publicly distribute the Work under its
terms, with knowledge of his or her Copyright and Related Rights in the
Work and the meaning and intended legal effect of CC0 on those rights.

1. Copyright and Related Rights. A Work made available under CC0 may be
protected by copyright and related or neighboring rights ("Copyright and
Related Rights"). Copyright and Related Rights include, but are not
limited to, the following:

  i. the right to reproduce, adapt, distribute, perform, display,
     communicate, and translate a Work;
 ii. moral rights retained by the original author(s) and/or performer(s);
iii. publicity and privacy rights pertaining to a person's image or
     likeness depicted in a Work;
 iv. rights protecting against unfair competition in regards to a Work,
     subject to the limitations in paragraph 4(a), below;
  v. rights protecting the extraction, dissemination, use and reuse of data
     in a Work;
 vi. database rights (such as those arising under Directive 96/9/EC of the
     European Parliament and of the Council of 11 March 1996 on the legal
     protection of databases, and under any national implementation
     thereof, including any amended or successor version of such
     directive); and
vii. other similar, equivalent or corresponding rights throughout the
     world based on applicable law or treaty, and any national
     implementations thereof.

2. Waiver. To the greatest extent permitted by, but not in contravention
of, applicable law, Affirmer hereby overtly, fully, permanently,
irrevocably and unconditionally waives, abandons, and surrenders all of
Affirmer's Copyright and Related Rights and associated claims and causes
of action, whether now known or unknown (including existing as well as
future claims and causes of action), in the Work (i) in all territories
worldwide, (ii) for the maximum duration provided by applicable law or
treaty (including future time extensions), (iii) in any current or future
medium and for any number of copies, and (iv) for any purpose whatsoever,
including without limitation commercial, advertising or promotional
purposes (the "Waiver"). Affirmer makes the Waiver for the benefit of each
member of the public at large and to the detriment of Affirmer's heirs and
successors, fully intending that such Waiver shall not be subject to
revocation, rescission, cancellation, termination, or any other legal or
equitable action to disrupt the quiet enjoyment of the Work by the public
as contemplated by Affirmer's express Statement of Purpose.

3. Public License Fallback. Should any part of the Waiver for any reason
be judged legally invalid or ineffective under applicable law, then the
Waiver shall be preserved to the maximum extent permitted taking into
account Affirmer's express Statement of Purpose. In addition, to the
extent the Waiver is so judged Affirmer hereby grants to each affected
person a royalty-free, non transferable, non sublicensable, non exclusive,
irrevocable and unconditional license to exercise Affirmer's Copyright and
Related Rights in the Work (i) in all territories worldwide, (ii) for the
maximum duration provided by applicable law or treaty (including future
time extensions), (iii) in any current or future medium and for any number
of copies, and (iv) for any purpose whatsoever, including without
limitation commercial, advertising or promotional purposes (the
"License"). The License shall be deemed effective as of the date CC0 was
applied by Affirmer to the Work. Should any part of the License for any
reason be judged legally invalid or ineffective under applicable law, such
partial invalidity or ineffectiveness shall not invalidate the remainder
of the License, and in such case Affirmer hereby affirms that he or she
will not (i) exercise any of his or her remaining Copyright and Related
Rights in the Work or (ii) assert any associated claims and causes of
action with respect to the Work, in either case contrary to Affirmer's
express Statement of Purpose.

4. Limitations and Disclaimers.

 a. No trademark or patent rights held by Affirmer are waived, abandoned,
    surrendered, licensed or otherwise affected by this document.
 b. Affirmer offers the Work as-is and makes no representations or
    warranties of any kind concerning the Work, express, implied,
    statutory or otherwise, including without limitation warranties of
    title, merchantability, fitness for a particular purpose, non
    infringement, or the absence of latent or other defects, accuracy, or
    the present or absence of errors, whether or not discoverable, all to
    the greatest extent permissible under applicable law.
 c. Affirmer disclaims responsibility for clearing rights of other persons
    that may apply to the Work or any use thereof, including without
    limitation any person's Copyright and Related Rights in the Work.
    Further, Affirmer disclaims responsibility for obtaining any necessary
    consents, permissions or other rights required for any use of the
    Work.
 d. Affirmer understands and acknowledges that Creative Commons is not a
    party to this document and has no duty or obligation with respect to
    this CC0 or use of the Work.
````

### Notice 8

````text
This project is licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
   https://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or
   https://opensource.org/licenses/MIT)

at your option.
````

### Notice 9

````text
The MIT License (MIT)

Copyright (c) 2018-2019 Andrew Gallant

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
````

### Notice 10

````text
Copyright (c) 2019 Nick Fitzgerald

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
````

### Notice 11

````text
Copyright (c) 2014 Alex Crichton

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
````

### Notice 12

````text
Permission is hereby granted, free of charge, to any person obtaining a
copy of this software and associated documentation files (the
"Software"), to deal in the Software without restriction, including
without limitation the rights to use, copy, modify, merge, publish,
distribute, sublicense, and/or sell copies of the Software, and to
permit persons to whom the Software is furnished to do so.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
````

### Notice 13

````text
Copyright (c) 2020-2026 The RustCrypto Project Developers

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
````

### Notice 14

````text
The MIT License (MIT)

Copyright (c) 2019 The Crossbeam Project Developers

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
````

### Notice 15

````text
Copyright (c) 2018-2019 dirs-rs contributors

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
````

### Notice 16

````text
Copyright (c) 2016--2023

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
````

### Notice 17

````text
Copyright (c) 2014 Chris Wong

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
````

### Notice 18

````text
Copyright (c) 2015 The rust-openssl-verify Developers

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
````

### Notice 19

````text
Copyright (c) 2016 The fallible-streaming-iterator Developers

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
````

### Notice 20

````text
Copyright (c) 2024 Orson Peters

This software is provided 'as-is', without any express or implied warranty. In
no event will the authors be held liable for any damages arising from the use of
this software.

Permission is granted to anyone to use this software for any purpose, including
commercial applications, and to alter it and redistribute it freely, subject to
the following restrictions:

1. The origin of this software must not be misrepresented; you must not claim
    that you wrote the original software. If you use this software in a product,
    an acknowledgment in the product documentation would be appreciated but is
    not required.

2. Altered source versions must be plainly marked as such, and must not be
    misrepresented as being the original software.

3. This notice may not be removed or altered from any source distribution.
````

### Notice 21

````text
Copyright (c) 2015 The Rust Project Developers

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
````

### Notice 22

````text
Copyright (c) 2018-2026 The rust-random Project Developers
Copyright (c) 2014 The Rust Project Developers

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
````

### Notice 23

````text
Copyright (c) 2016 Amanieu d'Antras

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
````

### Notice 24

````text
This work is derived in part from the `linked-hash-map` crate, Copyright (c)
2015 The Rust Project Developers

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
````

### Notice 25

````text
Copyright (c) 2016--2017

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
````

### Notice 26

````text
Copyright (c) Hanno Braun and contributors

Permission to use, copy, modify, and/or distribute this software for any purpose
with or without fee is hereby granted, provided that the above copyright notice
and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS
OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER
TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF
THIS SOFTWARE.
````

### Notice 27

````text
Copyright (c) 2016 William Orr <will@worrbase.com>

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
````

### Notice 28

````text
Copyright (c) The Rust Project Developers

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
````

### Notice 29

````text
MIT License

Copyright (c) 2023 4lDO2

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
````

### Notice 30

````text
Copyright (c) 2014 The rusqlite developers

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
````

### Notice 31

````text
Copyright (c) 2014 Carl Lerche and other MIO contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
````

### Notice 32

````text
Creative Commons CC0 1.0 Universal

<<beginOptional;name=ccOptionalIntro>> CREATIVE COMMONS CORPORATION IS NOT A LAW FIRM AND DOES NOT PROVIDE LEGAL SERVICES. DISTRIBUTION OF THIS DOCUMENT DOES NOT CREATE AN ATTORNEY-CLIENT RELATIONSHIP. CREATIVE COMMONS PROVIDES THIS INFORMATION ON AN "AS-IS" BASIS. CREATIVE COMMONS MAKES NO WARRANTIES REGARDING THE USE OF THIS DOCUMENT OR THE INFORMATION OR WORKS PROVIDED HEREUNDER, AND DISCLAIMS LIABILITY FOR DAMAGES RESULTING FROM THE USE OF THIS DOCUMENT OR THE INFORMATION OR WORKS PROVIDED HEREUNDER.  <<endOptional>>

Statement of Purpose

The laws of most jurisdictions throughout the world automatically confer exclusive Copyright and Related Rights (defined below) upon the creator and subsequent owner(s) (each and all, an "owner") of an original work of authorship and/or a database (each, a "Work").

Certain owners wish to permanently relinquish those rights to a Work for the purpose of contributing to a commons of creative, cultural and scientific works ("Commons") that the public can reliably and without fear of later claims of infringement build upon, modify, incorporate in other works, reuse and redistribute as freely as possible in any form whatsoever and for any purposes, including without limitation commercial purposes. These owners may contribute to the Commons to promote the ideal of a free culture and the further production of creative, cultural and scientific works, or to gain reputation or greater distribution for their Work in part through the use and efforts of others.

For these and/or other purposes and motivations, and without any expectation of additional consideration or compensation, the person associating CC0 with a Work (the "Affirmer"), to the extent that he or she is an owner of Copyright and Related Rights in the Work, voluntarily elects to apply CC0 to the Work and publicly distribute the Work under its terms, with knowledge of his or her Copyright and Related Rights in the Work and the meaning and intended legal effect of CC0 on those rights.

1. Copyright and Related Rights. A Work made available under CC0 may be protected by copyright and related or neighboring rights ("Copyright and Related Rights"). Copyright and Related Rights include, but are not limited to, the following:

     i. the right to reproduce, adapt, distribute, perform, display, communicate, and translate a Work;

     ii. moral rights retained by the original author(s) and/or performer(s);

     iii. publicity and privacy rights pertaining to a person's image or likeness depicted in a Work;

     iv. rights protecting against unfair competition in regards to a Work, subject to the limitations in paragraph 4(a), below;

     v. rights protecting the extraction, dissemination, use and reuse of data in a Work;

     vi. database rights (such as those arising under Directive 96/9/EC of the European Parliament and of the Council of 11 March 1996 on the legal protection of databases, and under any national implementation thereof, including any amended or successor version of such directive); and

     vii. other similar, equivalent or corresponding rights throughout the world based on applicable law or treaty, and any national implementations thereof.

2. Waiver. To the greatest extent permitted by, but not in contravention of, applicable law, Affirmer hereby overtly, fully, permanently, irrevocably and unconditionally waives, abandons, and surrenders all of Affirmer's Copyright and Related Rights and associated claims and causes of action, whether now known or unknown (including existing as well as future claims and causes of action), in the Work (i) in all territories worldwide, (ii) for the maximum duration provided by applicable law or treaty (including future time extensions), (iii) in any current or future medium and for any number of copies, and (iv) for any purpose whatsoever, including without limitation commercial, advertising or promotional purposes (the "Waiver"). Affirmer makes the Waiver for the benefit of each member of the public at large and to the detriment of Affirmer's heirs and successors, fully intending that such Waiver shall not be subject to revocation, rescission, cancellation, termination, or any other legal or equitable action to disrupt the quiet enjoyment of the Work by the public as contemplated by Affirmer's express Statement of Purpose.

3. Public License Fallback. Should any part of the Waiver for any reason be judged legally invalid or ineffective under applicable law, then the Waiver shall be preserved to the maximum extent permitted taking into account Affirmer's express Statement of Purpose. In addition, to the extent the Waiver is so judged Affirmer hereby grants to each affected person a royalty-free, non transferable, non sublicensable, non exclusive, irrevocable and unconditional license to exercise Affirmer's Copyright and Related Rights in the Work (i) in all territories worldwide, (ii) for the maximum duration provided by applicable law or treaty (including future time extensions), (iii) in any current or future medium and for any number of copies, and (iv) for any purpose whatsoever, including without limitation commercial, advertising or promotional purposes (the "License"). The License shall be deemed effective as of the date CC0 was applied by Affirmer to the Work. Should any part of the License for any reason be judged legally invalid or ineffective under applicable law, such partial invalidity or ineffectiveness shall not invalidate the remainder of the License, and in such case Affirmer hereby affirms that he or she will not (i) exercise any of his or her remaining Copyright and Related Rights in the Work or (ii) assert any associated claims and causes of action with respect to the Work, in either case contrary to Affirmer's express Statement of Purpose.

4. Limitations and Disclaimers.

     a. No trademark or patent rights held by Affirmer are waived, abandoned, surrendered, licensed or otherwise affected by this document.

     b. Affirmer offers the Work as-is and makes no representations or warranties of any kind concerning the Work, express, implied, statutory or otherwise, including without limitation warranties of title, merchantability, fitness for a particular purpose, non infringement, or the absence of latent or other defects, accuracy, or the present or absence of errors, whether or not discoverable, all to the greatest extent permissible under applicable law.

     c. Affirmer disclaims responsibility for clearing rights of other persons that may apply to the Work or any use thereof, including without limitation any person's Copyright and Related Rights in the Work. Further, Affirmer disclaims responsibility for obtaining any necessary consents, permissions or other rights required for any use of the Work.

     d. Affirmer understands and acknowledges that Creative Commons is not a party to this document and has no duty or obligation with respect to this CC0 or use of the Work.
````

### Notice 33

````text
Copyright (c) 2023 Notify Contributors

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
````

### Notice 34

````text
# License

The licensing of these crates is a bit complicated:
- The crates `objc2`, `block2`, `objc2-foundation` and `objc2-encode` are
  [currently][#23] licensed under [the MIT license][MIT].
- All other crates are trio-licensed under the [Zlib], [Apache-2.0] or [MIT]
  license, at your option.

Furthermore, the crates are (usually automatically) derived from Apple SDKs,
and that may have implications for licensing, see below for details.

[#23]: https://github.com/madsmtm/objc2/issues/23
[MIT]: https://opensource.org/license/MIT
[Zlib]: https://zlib.net/zlib_license.html
[Apache-2.0]: https://www.apache.org/licenses/LICENSE-2.0


## Apple SDKs

These crates are derived from Apple SDKs shipped with Xcode. You can obtain a
copy of the Xcode license at:

https://www.apple.com/legal/sla/docs/xcode.pdf

Or by typing `xcodebuild -license` in your terminal.

From reading the license, it is unclear whether distributing derived works
such as these crates are allowed?

But in any case, to practically use these crates, you will have to link, and
that only works when you have the correct Xcode SDK available to provide the
required `.tbd` files, which is why we choose to still use the normal SPDX
identifiers in the crates (Xcode is required to use the crates, and when using
Xcode you have already agreed to the Xcode license).
````

### Notice 35

````text
Apache License
                           Version 2.0, January 2004
                        http://www.apache.org/licenses/

   TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

   1. Definitions.

      "License" shall mean the terms and conditions for use, reproduction,
      and distribution as defined by Sections 1 through 9 of this document.

      "Licensor" shall mean the copyright owner or entity authorized by
      the copyright owner that is granting the License.

      "Legal Entity" shall mean the union of the acting entity and all
      other entities that control, are controlled by, or are under common
      control with that entity. For the purposes of this definition,
      "control" means (i) the power, direct or indirect, to cause the
      direction or management of such entity, whether by contract or
      otherwise, or (ii) ownership of fifty percent (50%) or more of the
      outstanding shares, or (iii) beneficial ownership of such entity.

      "You" (or "Your") shall mean an individual or Legal Entity
      exercising permissions granted by this License.

      "Source" form shall mean the preferred form for making modifications,
      including but not limited to software source code, documentation
      source, and configuration files.

      "Object" form shall mean any form resulting from mechanical
      transformation or translation of a Source form, including but
      not limited to compiled object code, generated documentation,
      and conversions to other media types.

      "Work" shall mean the work of authorship, whether in Source or
      Object form, made available under the License, as indicated by a
      copyright notice that is included in or attached to the work
      (an example is provided in the Appendix below).

      "Derivative Works" shall mean any work, whether in Source or Object
      form, that is based on (or derived from) the Work and for which the
      editorial revisions, annotations, elaborations, or other modifications
      represent, as a whole, an original work of authorship. For the purposes
      of this License, Derivative Works shall not include works that remain
      separable from, or merely link (or bind by name) to the interfaces of,
      the Work and Derivative Works thereof.

      "Contribution" shall mean any work of authorship, including
      the original version of the Work and any modifications or additions
      to that Work or Derivative Works thereof, that is intentionally
      submitted to Licensor for inclusion in the Work by the copyright owner
      or by an individual or Legal Entity authorized to submit on behalf of
      the copyright owner. For the purposes of this definition, "submitted"
      means any form of electronic, verbal, or written communication sent
      to the Licensor or its representatives, including but not limited to
      communication on electronic mailing lists, source code control systems,
      and issue tracking systems that are managed by, or on behalf of, the
      Licensor for the purpose of discussing and improving the Work, but
      excluding communication that is conspicuously marked or otherwise
      designated in writing by the copyright owner as "Not a Contribution."

      "Contributor" shall mean Licensor and any individual or Legal Entity
      on behalf of whom a Contribution has been received by Licensor and
      subsequently incorporated within the Work.

   2. Grant of Copyright License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      copyright license to reproduce, prepare Derivative Works of,
      publicly display, publicly perform, sublicense, and distribute the
      Work and such Derivative Works in Source or Object form.

   3. Grant of Patent License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      (except as stated in this section) patent license to make, have made,
      use, offer to sell, sell, import, and otherwise transfer the Work,
      where such license applies only to those patent claims licensable
      by such Contributor that are necessarily infringed by their
      Contribution(s) alone or by combination of their Contribution(s)
      with the Work to which such Contribution(s) was submitted. If You
      institute patent litigation against any entity (including a
      cross-claim or counterclaim in a lawsuit) alleging that the Work
      or a Contribution incorporated within the Work constitutes direct
      or contributory patent infringement, then any patent licenses
      granted to You under this License for that Work shall terminate
      as of the date such litigation is filed.

   4. Redistribution. You may reproduce and distribute copies of the
      Work or Derivative Works thereof in any medium, with or without
      modifications, and in Source or Object form, provided that You
      meet the following conditions:

      (a) You must give any other recipients of the Work or
          Derivative Works a copy of this License; and

      (b) You must cause any modified files to carry prominent notices
          stating that You changed the files; and

      (c) You must retain, in the Source form of any Derivative Works
          that You distribute, all copyright, patent, trademark, and
          attribution notices from the Source form of the Work,
          excluding those notices that do not pertain to any part of
          the Derivative Works; and

      (d) If the Work includes a "NOTICE" text file as part of its
          distribution, then any Derivative Works that You distribute must
          include a readable copy of the attribution notices contained
          within such NOTICE file, excluding those notices that do not
          pertain to any part of the Derivative Works, in at least one
          of the following places: within a NOTICE text file distributed
          as part of the Derivative Works; within the Source form or
          documentation, if provided along with the Derivative Works; or,
          within a display generated by the Derivative Works, if and
          wherever such third-party notices normally appear. The contents
          of the NOTICE file are for informational purposes only and
          do not modify the License. You may add Your own attribution
          notices within Derivative Works that You distribute, alongside
          or as an addendum to the NOTICE text from the Work, provided
          that such additional attribution notices cannot be construed
          as modifying the License.

      You may add Your own copyright statement to Your modifications and
      may provide additional or different license terms and conditions
      for use, reproduction, or distribution of Your modifications, or
      for any such Derivative Works as a whole, provided Your use,
      reproduction, and distribution of the Work otherwise complies with
      the conditions stated in this License.

   5. Submission of Contributions. Unless You explicitly state otherwise,
      any Contribution intentionally submitted for inclusion in the Work
      by You to the Licensor shall be under the terms and conditions of
      this License, without any additional terms or conditions.
      Notwithstanding the above, nothing herein shall supersede or modify
      the terms of any separate license agreement you may have executed
      with Licensor regarding such Contributions.

   6. Trademarks. This License does not grant permission to use the trade
      names, trademarks, service marks, or product names of the Licensor,
      except as required for reasonable and customary use in describing the
      origin of the Work and reproducing the content of the NOTICE file.

   7. Disclaimer of Warranty. Unless required by applicable law or
      agreed to in writing, Licensor provides the Work (and each
      Contributor provides its Contributions) on an "AS IS" BASIS,
      WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
      implied, including, without limitation, any warranties or conditions
      of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
      PARTICULAR PURPOSE. You are solely responsible for determining the
      appropriateness of using or redistributing the Work and assume any
      risks associated with Your exercise of permissions under this License.

   8. Limitation of Liability. In no event and under no legal theory,
      whether in tort (including negligence), contract, or otherwise,
      unless required by applicable law (such as deliberate and grossly
      negligent acts) or agreed to in writing, shall any Contributor be
      liable to You for damages, including any direct, indirect, special,
      incidental, or consequential damages of any character arising as a
      result of this License or out of the use or inability to use the
      Work (including but not limited to damages for loss of goodwill,
      work stoppage, computer failure or malfunction, or any and all
      other commercial damages or losses), even if such Contributor
      has been advised of the possibility of such damages.

   9. Accepting Warranty or Additional Liability. While redistributing
      the Work or Derivative Works thereof, You may choose to offer,
      and charge a fee for, acceptance of support, warranty, indemnity,
      or other liability obligations and/or rights consistent with this
      License. However, in accepting such obligations, You may act only
      on Your own behalf and on Your sole responsibility, not on behalf
      of any other Contributor, and only if You agree to indemnify,
      defend, and hold each Contributor harmless for any liability
      incurred by, or claims asserted against, such Contributor by reason
      of your accepting any such warranty or additional liability.

   END OF TERMS AND CONDITIONS

   APPENDIX: How to apply the Apache License to your work.

      To apply the Apache License to your work, attach the following
      boilerplate notice, with the fields enclosed by brackets "{}"
      replaced with your own identifying information. (Don't include
      the brackets!)  The text should be enclosed in the appropriate
      comment syntax for the file format. We also recommend that a
      file or class name and description of purpose be included on the
      same "printed page" as the copyright notice for easier
      identification within third-party archives.

   Copyright {yyyy} {name of copyright owner}

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

       http://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.
````

### Notice 36

````text
Mozilla Public License Version 2.0
==================================

1. Definitions
--------------

1.1. "Contributor"
    means each individual or legal entity that creates, contributes to
    the creation of, or owns Covered Software.

1.2. "Contributor Version"
    means the combination of the Contributions of others (if any) used
    by a Contributor and that particular Contributor's Contribution.

1.3. "Contribution"
    means Covered Software of a particular Contributor.

1.4. "Covered Software"
    means Source Code Form to which the initial Contributor has attached
    the notice in Exhibit A, the Executable Form of such Source Code
    Form, and Modifications of such Source Code Form, in each case
    including portions thereof.

1.5. "Incompatible With Secondary Licenses"
    means

    (a) that the initial Contributor has attached the notice described
        in Exhibit B to the Covered Software; or

    (b) that the Covered Software was made available under the terms of
        version 1.1 or earlier of the License, but not also under the
        terms of a Secondary License.

1.6. "Executable Form"
    means any form of the work other than Source Code Form.

1.7. "Larger Work"
    means a work that combines Covered Software with other material, in
    a separate file or files, that is not Covered Software.

1.8. "License"
    means this document.

1.9. "Licensable"
    means having the right to grant, to the maximum extent possible,
    whether at the time of the initial grant or subsequently, any and
    all of the rights conveyed by this License.

1.10. "Modifications"
    means any of the following:

    (a) any file in Source Code Form that results from an addition to,
        deletion from, or modification of the contents of Covered
        Software; or

    (b) any new file in Source Code Form that contains any Covered
        Software.

1.11. "Patent Claims" of a Contributor
    means any patent claim(s), including without limitation, method,
    process, and apparatus claims, in any patent Licensable by such
    Contributor that would be infringed, but for the grant of the
    License, by the making, using, selling, offering for sale, having
    made, import, or transfer of either its Contributions or its
    Contributor Version.

1.12. "Secondary License"
    means either the GNU General Public License, Version 2.0, the GNU
    Lesser General Public License, Version 2.1, the GNU Affero General
    Public License, Version 3.0, or any later versions of those
    licenses.

1.13. "Source Code Form"
    means the form of the work preferred for making modifications.

1.14. "You" (or "Your")
    means an individual or a legal entity exercising rights under this
    License. For legal entities, "You" includes any entity that
    controls, is controlled by, or is under common control with You. For
    purposes of this definition, "control" means (a) the power, direct
    or indirect, to cause the direction or management of such entity,
    whether by contract or otherwise, or (b) ownership of more than
    fifty percent (50%) of the outstanding shares or beneficial
    ownership of such entity.

2. License Grants and Conditions
--------------------------------

2.1. Grants

Each Contributor hereby grants You a world-wide, royalty-free,
non-exclusive license:

(a) under intellectual property rights (other than patent or trademark)
    Licensable by such Contributor to use, reproduce, make available,
    modify, display, perform, distribute, and otherwise exploit its
    Contributions, either on an unmodified basis, with Modifications, or
    as part of a Larger Work; and

(b) under Patent Claims of such Contributor to make, use, sell, offer
    for sale, have made, import, and otherwise transfer either its
    Contributions or its Contributor Version.

2.2. Effective Date

The licenses granted in Section 2.1 with respect to any Contribution
become effective for each Contribution on the date the Contributor first
distributes such Contribution.

2.3. Limitations on Grant Scope

The licenses granted in this Section 2 are the only rights granted under
this License. No additional rights or licenses will be implied from the
distribution or licensing of Covered Software under this License.
Notwithstanding Section 2.1(b) above, no patent license is granted by a
Contributor:

(a) for any code that a Contributor has removed from Covered Software;
    or

(b) for infringements caused by: (i) Your and any other third party's
    modifications of Covered Software, or (ii) the combination of its
    Contributions with other software (except as part of its Contributor
    Version); or

(c) under Patent Claims infringed by Covered Software in the absence of
    its Contributions.

This License does not grant any rights in the trademarks, service marks,
or logos of any Contributor (except as may be necessary to comply with
the notice requirements in Section 3.4).

2.4. Subsequent Licenses

No Contributor makes additional grants as a result of Your choice to
distribute the Covered Software under a subsequent version of this
License (see Section 10.2) or under the terms of a Secondary License (if
permitted under the terms of Section 3.3).

2.5. Representation

Each Contributor represents that the Contributor believes its
Contributions are its original creation(s) or it has sufficient rights
to grant the rights to its Contributions conveyed by this License.

2.6. Fair Use

This License is not intended to limit any rights You have under
applicable copyright doctrines of fair use, fair dealing, or other
equivalents.

2.7. Conditions

Sections 3.1, 3.2, 3.3, and 3.4 are conditions of the licenses granted
in Section 2.1.

3. Responsibilities
-------------------

3.1. Distribution of Source Form

All distribution of Covered Software in Source Code Form, including any
Modifications that You create or to which You contribute, must be under
the terms of this License. You must inform recipients that the Source
Code Form of the Covered Software is governed by the terms of this
License, and how they can obtain a copy of this License. You may not
attempt to alter or restrict the recipients' rights in the Source Code
Form.

3.2. Distribution of Executable Form

If You distribute Covered Software in Executable Form then:

(a) such Covered Software must also be made available in Source Code
    Form, as described in Section 3.1, and You must inform recipients of
    the Executable Form how they can obtain a copy of such Source Code
    Form by reasonable means in a timely manner, at a charge no more
    than the cost of distribution to the recipient; and

(b) You may distribute such Executable Form under the terms of this
    License, or sublicense it under different terms, provided that the
    license for the Executable Form does not attempt to limit or alter
    the recipients' rights in the Source Code Form under this License.

3.3. Distribution of a Larger Work

You may create and distribute a Larger Work under terms of Your choice,
provided that You also comply with the requirements of this License for
the Covered Software. If the Larger Work is a combination of Covered
Software with a work governed by one or more Secondary Licenses, and the
Covered Software is not Incompatible With Secondary Licenses, this
License permits You to additionally distribute such Covered Software
under the terms of such Secondary License(s), so that the recipient of
the Larger Work may, at their option, further distribute the Covered
Software under the terms of either this License or such Secondary
License(s).

3.4. Notices

You may not remove or alter the substance of any license notices
(including copyright notices, patent notices, disclaimers of warranty,
or limitations of liability) contained within the Source Code Form of
the Covered Software, except that You may alter any license notices to
the extent required to remedy known factual inaccuracies.

3.5. Application of Additional Terms

You may choose to offer, and to charge a fee for, warranty, support,
indemnity or liability obligations to one or more recipients of Covered
Software. However, You may do so only on Your own behalf, and not on
behalf of any Contributor. You must make it absolutely clear that any
such warranty, support, indemnity, or liability obligation is offered by
You alone, and You hereby agree to indemnify every Contributor for any
liability incurred by such Contributor as a result of warranty, support,
indemnity or liability terms You offer. You may include additional
disclaimers of warranty and limitations of liability specific to any
jurisdiction.

4. Inability to Comply Due to Statute or Regulation
---------------------------------------------------

If it is impossible for You to comply with any of the terms of this
License with respect to some or all of the Covered Software due to
statute, judicial order, or regulation then You must: (a) comply with
the terms of this License to the maximum extent possible; and (b)
describe the limitations and the code they affect. Such description must
be placed in a text file included with all distributions of the Covered
Software under this License. Except to the extent prohibited by statute
or regulation, such description must be sufficiently detailed for a
recipient of ordinary skill to be able to understand it.

5. Termination
--------------

5.1. The rights granted under this License will terminate automatically
if You fail to comply with any of its terms. However, if You become
compliant, then the rights granted under this License from a particular
Contributor are reinstated (a) provisionally, unless and until such
Contributor explicitly and finally terminates Your grants, and (b) on an
ongoing basis, if such Contributor fails to notify You of the
non-compliance by some reasonable means prior to 60 days after You have
come back into compliance. Moreover, Your grants from a particular
Contributor are reinstated on an ongoing basis if such Contributor
notifies You of the non-compliance by some reasonable means, this is the
first time You have received notice of non-compliance with this License
from such Contributor, and You become compliant prior to 30 days after
Your receipt of the notice.

5.2. If You initiate litigation against any entity by asserting a patent
infringement claim (excluding declaratory judgment actions,
counter-claims, and cross-claims) alleging that a Contributor Version
directly or indirectly infringes any patent, then the rights granted to
You by any and all Contributors for the Covered Software under Section
2.1 of this License shall terminate.

5.3. In the event of termination under Sections 5.1 or 5.2 above, all
end user license agreements (excluding distributors and resellers) which
have been validly granted by You or Your distributors under this License
prior to termination shall survive termination.

************************************************************************
*                                                                      *
*  6. Disclaimer of Warranty                                           *
*  -------------------------                                           *
*                                                                      *
*  Covered Software is provided under this License on an "as is"       *
*  basis, without warranty of any kind, either expressed, implied, or  *
*  statutory, including, without limitation, warranties that the       *
*  Covered Software is free of defects, merchantable, fit for a        *
*  particular purpose or non-infringing. The entire risk as to the     *
*  quality and performance of the Covered Software is with You.        *
*  Should any Covered Software prove defective in any respect, You     *
*  (not any Contributor) assume the cost of any necessary servicing,   *
*  repair, or correction. This disclaimer of warranty constitutes an   *
*  essential part of this License. No use of any Covered Software is   *
*  authorized under this License except under this disclaimer.         *
*                                                                      *
************************************************************************

************************************************************************
*                                                                      *
*  7. Limitation of Liability                                          *
*  --------------------------                                          *
*                                                                      *
*  Under no circumstances and under no legal theory, whether tort      *
*  (including negligence), contract, or otherwise, shall any           *
*  Contributor, or anyone who distributes Covered Software as          *
*  permitted above, be liable to You for any direct, indirect,         *
*  special, incidental, or consequential damages of any character      *
*  including, without limitation, damages for lost profits, loss of    *
*  goodwill, work stoppage, computer failure or malfunction, or any    *
*  and all other commercial damages or losses, even if such party      *
*  shall have been informed of the possibility of such damages. This   *
*  limitation of liability shall not apply to liability for death or   *
*  personal injury resulting from such party's negligence to the       *
*  extent applicable law prohibits such limitation. Some               *
*  jurisdictions do not allow the exclusion or limitation of           *
*  incidental or consequential damages, so this exclusion and          *
*  limitation may not apply to You.                                    *
*                                                                      *
************************************************************************

8. Litigation
-------------

Any litigation relating to this License may be brought only in the
courts of a jurisdiction where the defendant maintains its principal
place of business and such litigation shall be governed by laws of that
jurisdiction, without reference to its conflict-of-law provisions.
Nothing in this Section shall prevent a party's ability to bring
cross-claims or counter-claims.

9. Miscellaneous
----------------

This License represents the complete agreement concerning the subject
matter hereof. If any provision of this License is held to be
unenforceable, such provision shall be reformed only to the extent
necessary to make it enforceable. Any law or regulation which provides
that the language of a contract shall be construed against the drafter
shall not be used to construe this License against a Contributor.

10. Versions of the License
---------------------------

10.1. New Versions

Mozilla Foundation is the license steward. Except as provided in Section
10.3, no one other than the license steward has the right to modify or
publish new versions of this License. Each version will be given a
distinguishing version number.

10.2. Effect of New Versions

You may distribute the Covered Software under the terms of the version
of the License under which You originally received the Covered Software,
or under the terms of any subsequent version published by the license
steward.

10.3. Modified Versions

If you create software not governed by this License, and you want to
create a new license for such software, you may create and use a
modified version of this License if you rename the license and remove
any references to the name of the license steward (except to note that
such modified license differs from this License).

10.4. Distributing Source Code Form that is Incompatible With Secondary
Licenses

If You choose to distribute Source Code Form that is Incompatible With
Secondary Licenses under the terms of this version of the License, the
notice described in Exhibit B of this License must be attached.

Exhibit A - Source Code Form License Notice
-------------------------------------------

  This Source Code Form is subject to the terms of the Mozilla Public
  License, v. 2.0. If a copy of the MPL was not distributed with this
  file, You can obtain one at https://mozilla.org/MPL/2.0/.

If it is not possible or desirable to put the notice in a particular
file, then You may include the notice in a location (such as a LICENSE
file in a relevant directory) where a recipient would be likely to look
for such a notice.

You may add additional accurate notices of copyright ownership.

Exhibit B - "Incompatible With Secondary Licenses" Notice
---------------------------------------------------------

  This Source Code Form is "Incompatible With Secondary Licenses", as
  defined by the Mozilla Public License, v. 2.0.
````

### Notice 37

````text
LICENSE:
        This project is triple-licensed under the MIT License, the Apache
        License, Version 2.0, and the GNU Lesser General Public License,
        Version 2.1+.

AUTHORS-MIT:
        Permission is hereby granted, free of charge, to any person obtaining a
        copy of this software and associated documentation files (the
        "Software"), to deal in the Software without restriction, including
        without limitation the rights to use, copy, modify, merge, publish,
        distribute, sublicense, and/or sell copies of the Software, and to
        permit persons to whom the Software is furnished to do so, subject to
        the following conditions:

        The above copyright notice and this permission notice shall be included
        in all copies or substantial portions of the Software.

        THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
        OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
        MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
        IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
        CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
        TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
        SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

AUTHORS-ASL:
        Licensed under the Apache License, Version 2.0 (the "License");
        you may not use this file except in compliance with the License.
        You may obtain a copy of the License at

                http://www.apache.org/licenses/LICENSE-2.0

        Unless required by applicable law or agreed to in writing, software
        distributed under the License is distributed on an "AS IS" BASIS,
        WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
        See the License for the specific language governing permissions and
        limitations under the License.

AUTHORS-LGPL:
        This program is free software; you can redistribute it and/or modify it
        under the terms of the GNU Lesser General Public License as published
        by the Free Software Foundation; either version 2.1 of the License, or
        (at your option) any later version.

        This program is distributed in the hope that it will be useful, but
        WITHOUT ANY WARRANTY; without even the implied warranty of
        MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU
        Lesser General Public License for more details.

        You should have received a copy of the GNU Lesser General Public License
        along with this program; If not, see <http://www.gnu.org/licenses/>.

COPYRIGHT: (ordered alphabetically)
        Copyright (C) 2017-2023 Red Hat, Inc.
        Copyright (C) 2019-2023 Microsoft Corporation
        Copyright (C) 2022-2023 David Rheinsberg

AUTHORS: (ordered alphabetically)
        Alan Egerton <eggyal@gmail.com>
        Alex James <theracermaster@gmail.com>
        Ayush Singh <ayushsingh1325@gmail.com>
        Boris-Chengbiao Zhou <bobo1239@web.de>
        Bret Barkelew <bret@corthon.com>
        Christopher Zurcher <christopher.zurcher@microsoft.com>
        David Rheinsberg <david@readahead.eu>
        Dmitry Mostovenko <trueberserker@gmail.com>
        Hiroki Tokunaga <tokusan441@gmail.com>
        Joe Richey <joerichey@google.com>
        John Schock <joschock@microsoft.com>
        Michael Kubacki <michael.kubacki@microsoft.com>
        Oliver Smith-Denny <osde@microsoft.com>
        Richard Wiedenhöft <richard@wiedenhoeft.xyz>
        Rob Bradford <robert.bradford@intel.com>, <rbradford@rivosinc.com>
        Tom Gundersen <teg@jklm.no>
        Trevor Gross <tmgross@umich.edu>
````

### Notice 38

````text
The MIT License (MIT)

Copyright (c) 2017 Jose Narvaez

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
````

### Notice 39

````text
UNICODE, INC. LICENSE AGREEMENT - DATA FILES AND SOFTWARE

Unicode Data Files include all data files under the directories
http://www.unicode.org/Public/, http://www.unicode.org/reports/,
http://www.unicode.org/cldr/data/, http://source.icu-project.org/repos/icu/, and
http://www.unicode.org/utility/trac/browser/.

Unicode Data Files do not include PDF online code charts under the
directory http://www.unicode.org/Public/.

Software includes any source code published in the Unicode Standard
or under the directories
http://www.unicode.org/Public/, http://www.unicode.org/reports/,
http://www.unicode.org/cldr/data/, http://source.icu-project.org/repos/icu/, and
http://www.unicode.org/utility/trac/browser/.

NOTICE TO USER: Carefully read the following legal agreement.
BY DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING UNICODE INC.'S
DATA FILES ("DATA FILES"), AND/OR SOFTWARE ("SOFTWARE"),
YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE
TERMS AND CONDITIONS OF THIS AGREEMENT.
IF YOU DO NOT AGREE, DO NOT DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE
THE DATA FILES OR SOFTWARE.

COPYRIGHT AND PERMISSION NOTICE

Copyright © 1991-2018 Unicode, Inc. All rights reserved.
Distributed under the Terms of Use in http://www.unicode.org/copyright.html.

Permission is hereby granted, free of charge, to any person obtaining
a copy of the Unicode data files and any associated documentation
(the "Data Files") or Unicode software and any associated documentation
(the "Software") to deal in the Data Files or Software
without restriction, including without limitation the rights to use,
copy, modify, merge, publish, distribute, and/or sell copies of
the Data Files or Software, and to permit persons to whom the Data Files
or Software are furnished to do so, provided that either
(a) this copyright and permission notice appear with all copies
of the Data Files or Software, or
(b) this copyright and permission notice appear in associated
Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE
WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT OF THIRD PARTY RIGHTS.
IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS
NOTICE BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL
DAMAGES, OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE,
DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER
TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
PERFORMANCE OF THE DATA FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder
shall not be used in advertising or otherwise to promote the sale,
use or other dealings in these Data Files or Software without prior
written authorization of the copyright holder.
````

### Notice 40

````text
The MIT License (MIT)

Copyright (c) 2017 Andrew Gallant

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
````

### Notice 41

````text
The MIT License (MIT)

Copyright (c) 2015 Nicholas Allegra (comex).

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
````

### Notice 42

````text
Copyright (c) 2018 The Servo Project Developers

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
````

### Notice 43

````text
MIT License

Copyright (c) 2024 Spxg

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
````

### Notice 44

````text
Copyright (c) 2016 Steven Fackler

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
````

### Notice 45

````text
The MIT License (MIT)

Copyright (c) 2015 Danny Guo
Copyright (c) 2016 Titus Wormer <tituswormer@gmail.com>
Copyright (c) 2018 Akash Kurdekar

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
````

### Notice 46

````text
Copyright (c) 2015 Steven Allen

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
````

### Notice 47

````text
COPYRIGHT AND PERMISSION NOTICE (ICU 58 and later)

Copyright © 1991-2019 Unicode, Inc. All rights reserved.
Distributed under the Terms of Use in https://www.unicode.org/copyright.html.

Permission is hereby granted, free of charge, to any person obtaining
a copy of the Unicode data files and any associated documentation
(the "Data Files") or Unicode software and any associated documentation
(the "Software") to deal in the Data Files or Software
without restriction, including without limitation the rights to use,
copy, modify, merge, publish, distribute, and/or sell copies of
the Data Files or Software, and to permit persons to whom the Data Files
or Software are furnished to do so, provided that either
(a) this copyright and permission notice appear with all copies
of the Data Files or Software, or
(b) this copyright and permission notice appear in associated
Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE
WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT OF THIRD PARTY RIGHTS.
IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS
NOTICE BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL
DAMAGES, OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE,
DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER
TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
PERFORMANCE OF THE DATA FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder
shall not be used in advertising or otherwise to promote the sale,
use or other dealings in these Data Files or Software without prior
written authorization of the copyright holder.

---------------------

Third-Party Software Licenses

This section contains third-party software notices and/or additional
terms for licensed third-party software components included within ICU
libraries.

1. ICU License - ICU 1.8.1 to ICU 57.1

COPYRIGHT AND PERMISSION NOTICE

Copyright (c) 1995-2016 International Business Machines Corporation and others
All rights reserved.

Permission is hereby granted, free of charge, to any person obtaining
a copy of this software and associated documentation files (the
"Software"), to deal in the Software without restriction, including
without limitation the rights to use, copy, modify, merge, publish,
distribute, and/or sell copies of the Software, and to permit persons
to whom the Software is furnished to do so, provided that the above
copyright notice(s) and this permission notice appear in all copies of
the Software and that both the above copyright notice(s) and this
permission notice appear in supporting documentation.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT
OF THIRD PARTY RIGHTS. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR
HOLDERS INCLUDED IN THIS NOTICE BE LIABLE FOR ANY CLAIM, OR ANY
SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES, OR ANY DAMAGES WHATSOEVER
RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF
CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

Except as contained in this notice, the name of a copyright holder
shall not be used in advertising or otherwise to promote the sale, use
or other dealings in this Software without prior written authorization
of the copyright holder.

All trademarks and registered trademarks mentioned herein are the
property of their respective owners.

2. Chinese/Japanese Word Break Dictionary Data (cjdict.txt)

 #     The Google Chrome software developed by Google is licensed under
 # the BSD license. Other software included in this distribution is
 # provided under other licenses, as set forth below.
 #
 #  The BSD License
 #  http://opensource.org/licenses/bsd-license.php
 #  Copyright (C) 2006-2008, Google Inc.
 #
 #  All rights reserved.
 #
 #  Redistribution and use in source and binary forms, with or without
 # modification, are permitted provided that the following conditions are met:
 #
 #  Redistributions of source code must retain the above copyright notice,
 # this list of conditions and the following disclaimer.
 #  Redistributions in binary form must reproduce the above
 # copyright notice, this list of conditions and the following
 # disclaimer in the documentation and/or other materials provided with
 # the distribution.
 #  Neither the name of  Google Inc. nor the names of its
 # contributors may be used to endorse or promote products derived from
 # this software without specific prior written permission.
 #
 #
 #  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND
 # CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
 # INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF
 # MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 # DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE
 # LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 # CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 # SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR
 # BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
 # LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
 # NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
 # SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 #
 #
 #  The word list in cjdict.txt are generated by combining three word lists
 # listed below with further processing for compound word breaking. The
 # frequency is generated with an iterative training against Google web
 # corpora.
 #
 #  * Libtabe (Chinese)
 #    - https://sourceforge.net/project/?group_id=1519
 #    - Its license terms and conditions are shown below.
 #
 #  * IPADIC (Japanese)
 #    - http://chasen.aist-nara.ac.jp/chasen/distribution.html
 #    - Its license terms and conditions are shown below.
 #
 #  ---------COPYING.libtabe ---- BEGIN--------------------
 #
 #  /*
 #   * Copyright (c) 1999 TaBE Project.
 #   * Copyright (c) 1999 Pai-Hsiang Hsiao.
 #   * All rights reserved.
 #   *
 #   * Redistribution and use in source and binary forms, with or without
 #   * modification, are permitted provided that the following conditions
 #   * are met:
 #   *
 #   * . Redistributions of source code must retain the above copyright
 #   *   notice, this list of conditions and the following disclaimer.
 #   * . Redistributions in binary form must reproduce the above copyright
 #   *   notice, this list of conditions and the following disclaimer in
 #   *   the documentation and/or other materials provided with the
 #   *   distribution.
 #   * . Neither the name of the TaBE Project nor the names of its
 #   *   contributors may be used to endorse or promote products derived
 #   *   from this software without specific prior written permission.
 #   *
 #   * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
 #   * "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
 #   * LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
 #   * FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE
 #   * REGENTS OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
 #   * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 #   * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 #   * SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 #   * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
 #   * STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 #   * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED
 #   * OF THE POSSIBILITY OF SUCH DAMAGE.
 #   */
 #
 #  /*
 #   * Copyright (c) 1999 Computer Systems and Communication Lab,
 #   *                    Institute of Information Science, Academia
 #       *                    Sinica. All rights reserved.
 #   *
 #   * Redistribution and use in source and binary forms, with or without
 #   * modification, are permitted provided that the following conditions
 #   * are met:
 #   *
 #   * . Redistributions of source code must retain the above copyright
 #   *   notice, this list of conditions and the following disclaimer.
 #   * . Redistributions in binary form must reproduce the above copyright
 #   *   notice, this list of conditions and the following disclaimer in
 #   *   the documentation and/or other materials provided with the
 #   *   distribution.
 #   * . Neither the name of the Computer Systems and Communication Lab
 #   *   nor the names of its contributors may be used to endorse or
 #   *   promote products derived from this software without specific
 #   *   prior written permission.
 #   *
 #   * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
 #   * "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
 #   * LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
 #   * FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE
 #   * REGENTS OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
 #   * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 #   * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 #   * SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 #   * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
 #   * STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 #   * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED
 #   * OF THE POSSIBILITY OF SUCH DAMAGE.
 #   */
 #
 #  Copyright 1996 Chih-Hao Tsai @ Beckman Institute,
 #      University of Illinois
 #  c-tsai4@uiuc.edu  http://casper.beckman.uiuc.edu/~c-tsai4
 #
 #  ---------------COPYING.libtabe-----END--------------------------------
 #
 #
 #  ---------------COPYING.ipadic-----BEGIN-------------------------------
 #
 #  Copyright 2000, 2001, 2002, 2003 Nara Institute of Science
 #  and Technology.  All Rights Reserved.
 #
 #  Use, reproduction, and distribution of this software is permitted.
 #  Any copy of this software, whether in its original form or modified,
 #  must include both the above copyright notice and the following
 #  paragraphs.
 #
 #  Nara Institute of Science and Technology (NAIST),
 #  the copyright holders, disclaims all warranties with regard to this
 #  software, including all implied warranties of merchantability and
 #  fitness, in no event shall NAIST be liable for
 #  any special, indirect or consequential damages or any damages
 #  whatsoever resulting from loss of use, data or profits, whether in an
 #  action of contract, negligence or other tortuous action, arising out
 #  of or in connection with the use or performance of this software.
 #
 #  A large portion of the dictionary entries
 #  originate from ICOT Free Software.  The following conditions for ICOT
 #  Free Software applies to the current dictionary as well.
 #
 #  Each User may also freely distribute the Program, whether in its
 #  original form or modified, to any third party or parties, PROVIDED
 #  that the provisions of Section 3 ("NO WARRANTY") will ALWAYS appear
 #  on, or be attached to, the Program, which is distributed substantially
 #  in the same form as set out herein and that such intended
 #  distribution, if actually made, will neither violate or otherwise
 #  contravene any of the laws and regulations of the countries having
 #  jurisdiction over the User or the intended distribution itself.
 #
 #  NO WARRANTY
 #
 #  The program was produced on an experimental basis in the course of the
 #  research and development conducted during the project and is provided
 #  to users as so produced on an experimental basis.  Accordingly, the
 #  program is provided without any warranty whatsoever, whether express,
 #  implied, statutory or otherwise.  The term "warranty" used herein
 #  includes, but is not limited to, any warranty of the quality,
 #  performance, merchantability and fitness for a particular purpose of
 #  the program and the nonexistence of any infringement or violation of
 #  any right of any third party.
 #
 #  Each user of the program will agree and understand, and be deemed to
 #  have agreed and understood, that there is no warranty whatsoever for
 #  the program and, accordingly, the entire risk arising from or
 #  otherwise connected with the program is assumed by the user.
 #
 #  Therefore, neither ICOT, the copyright holder, or any other
 #  organization that participated in or was otherwise related to the
 #  development of the program and their respective officials, directors,
 #  officers and other employees shall be held liable for any and all
 #  damages, including, without limitation, general, special, incidental
 #  and consequential damages, arising out of or otherwise in connection
 #  with the use or inability to use the program or any product, material
 #  or result produced or otherwise obtained by using the program,
 #  regardless of whether they have been advised of, or otherwise had
 #  knowledge of, the possibility of such damages at any time during the
 #  project or thereafter.  Each user will be deemed to have agreed to the
 #  foregoing by his or her commencement of use of the program.  The term
 #  "use" as used herein includes, but is not limited to, the use,
 #  modification, copying and distribution of the program and the
 #  production of secondary products from the program.
 #
 #  In the case where the program, whether in its original form or
 #  modified, was distributed or delivered to or received by a user from
 #  any person, organization or entity other than ICOT, unless it makes or
 #  grants independently of ICOT any specific warranty to the user in
 #  writing, such person, organization or entity, will also be exempted
 #  from and not be held liable to the user for any such damages as noted
 #  above as far as the program is concerned.
 #
 #  ---------------COPYING.ipadic-----END----------------------------------

3. Lao Word Break Dictionary Data (laodict.txt)

 #  Copyright (c) 2013 International Business Machines Corporation
 #  and others. All Rights Reserved.
 #
 # Project: http://code.google.com/p/lao-dictionary/
 # Dictionary: http://lao-dictionary.googlecode.com/git/Lao-Dictionary.txt
 # License: http://lao-dictionary.googlecode.com/git/Lao-Dictionary-LICENSE.txt
 #              (copied below)
 #
 #  This file is derived from the above dictionary, with slight
 #  modifications.
 #  ----------------------------------------------------------------------
 #  Copyright (C) 2013 Brian Eugene Wilson, Robert Martin Campbell.
 #  All rights reserved.
 #
 #  Redistribution and use in source and binary forms, with or without
 #  modification,
 #  are permitted provided that the following conditions are met:
 #
 #
 # Redistributions of source code must retain the above copyright notice, this
 #  list of conditions and the following disclaimer. Redistributions in
 #  binary form must reproduce the above copyright notice, this list of
 #  conditions and the following disclaimer in the documentation and/or
 #  other materials provided with the distribution.
 #
 #
 # THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
 # "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
 # LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
 # FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE
 # COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT,
 # INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 # (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 # SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 # HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
 # STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 # ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED
 # OF THE POSSIBILITY OF SUCH DAMAGE.
 #  --------------------------------------------------------------------------

4. Burmese Word Break Dictionary Data (burmesedict.txt)

 #  Copyright (c) 2014 International Business Machines Corporation
 #  and others. All Rights Reserved.
 #
 #  This list is part of a project hosted at:
 #    github.com/kanyawtech/myanmar-karen-word-lists
 #
 #  --------------------------------------------------------------------------
 #  Copyright (c) 2013, LeRoy Benjamin Sharon
 #  All rights reserved.
 #
 #  Redistribution and use in source and binary forms, with or without
 #  modification, are permitted provided that the following conditions
 #  are met: Redistributions of source code must retain the above
 #  copyright notice, this list of conditions and the following
 #  disclaimer.  Redistributions in binary form must reproduce the
 #  above copyright notice, this list of conditions and the following
 #  disclaimer in the documentation and/or other materials provided
 #  with the distribution.
 #
 #    Neither the name Myanmar Karen Word Lists, nor the names of its
 #    contributors may be used to endorse or promote products derived
 #    from this software without specific prior written permission.
 #
 #  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND
 #  CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
 #  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF
 #  MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 #  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS
 #  BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
 #  EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED
 #  TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 #  DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON
 #  ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR
 #  TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF
 #  THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 #  SUCH DAMAGE.
 #  --------------------------------------------------------------------------

5. Time Zone Database

  ICU uses the public domain data and code derived from Time Zone
Database for its time zone support. The ownership of the TZ database
is explained in BCP 175: Procedure for Maintaining the Time Zone
Database section 7.

 # 7.  Database Ownership
 #
 #    The TZ database itself is not an IETF Contribution or an IETF
 #    document.  Rather it is a pre-existing and regularly updated work
 #    that is in the public domain, and is intended to remain in the
 #    public domain.  Therefore, BCPs 78 [RFC5378] and 79 [RFC3979] do
 #    not apply to the TZ Database or contributions that individuals make
 #    to it.  Should any claims be made and substantiated against the TZ
 #    Database, the organization that is providing the IANA
 #    Considerations defined in this RFC, under the memorandum of
 #    understanding with the IETF, currently ICANN, may act in accordance
 #    with all competent court orders.  No ownership claims will be made
 #    by ICANN or the IETF Trust on the database or the code.  Any person
 #    making a contribution to the database or code waives all rights to
 #    future claims in that contribution or in the TZ Database.

6. Google double-conversion

Copyright 2006-2011, the V8 project authors. All rights reserved.
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

    * Redistributions of source code must retain the above copyright
      notice, this list of conditions and the following disclaimer.
    * Redistributions in binary form must reproduce the above
      copyright notice, this list of conditions and the following
      disclaimer in the documentation and/or other materials provided
      with the distribution.
    * Neither the name of Google Inc. nor the names of its
      contributors may be used to endorse or promote products derived
      from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
````

### Notice 48

````text
The MIT License (MIT)

Copyright (c) 2018-2024 Max Brunsfeld

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
````

### Notice 49

````text
The MIT License (MIT)

Copyright (c) 2014 Max Brunsfeld

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
````

### Notice 50

````text
The MIT License (MIT)

Copyright (c) 2018 Max Brunsfeld

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
````

### Notice 51

````text
The MIT License (MIT)

Copyright (c) 2017 Maxim Sokolov

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
````

### Notice 52

````text
The MIT License (MIT)

Copyright (c) 2017 Max Brunsfeld

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
````

### Notice 53

````text
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
````

### Notice 54

````text
Copyright (c) 2016 Joe Wilm

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
````

### Notice 55

````text
Copyright (c) 2017 Jim McGrath

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
````

### Notice 56

````text
Copyright (c) 2015-2018 The winapi-rs Developers

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
````

### Notice 57

````text
MIT License

    Copyright (c) Microsoft Corporation.

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
    SOFTWARE
````

### Notice 58

```text
The MIT License (MIT)

Copyright (c) 2016 Rob Rix

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

### Notice 59

````text
Short version for non-lawyers:

`ambient-authority` is triple-licensed under Apache 2.0 with the LLVM Exception,
Apache 2.0, and MIT terms.


Longer version:

Copyrights in the `ambient-authority` project are retained by their contributors.
No copyright assignment is required to contribute to the `ambient-authority`
project.

Some files include code derived from Rust's `libstd`; see the comments in
the code for details.

Except as otherwise noted (below and/or in individual files), `ambient-authority`
is licensed under:

 - the Apache License, Version 2.0, with the LLVM Exception
   <LICENSE-Apache-2.0_WITH_LLVM-exception> or
   <http://llvm.org/foundation/relicensing/LICENSE.txt>
 - the Apache License, Version 2.0
   <LICENSE-APACHE> or
   <http://www.apache.org/licenses/LICENSE-2.0>,
 - or the MIT license
   <LICENSE-MIT> or
   <http://opensource.org/licenses/MIT>,

at your option.
````

### Notice 60

````text
Short version for non-lawyers:

`cap-fs-ext` is triple-licensed under Apache 2.0 with the LLVM Exception,
Apache 2.0, and MIT terms.


Longer version:

Copyrights in the `cap-fs-ext` project are retained by their contributors.
No copyright assignment is required to contribute to the `cap-fs-ext`
project.

Some files include code derived from Rust's `libstd`; see the comments in
the code for details.

Except as otherwise noted (below and/or in individual files), `cap-fs-ext`
is licensed under:

 - the Apache License, Version 2.0, with the LLVM Exception
   <LICENSE-Apache-2.0_WITH_LLVM-exception> or
   <http://llvm.org/foundation/relicensing/LICENSE.txt>
 - the Apache License, Version 2.0
   <LICENSE-APACHE> or
   <http://www.apache.org/licenses/LICENSE-2.0>,
 - or the MIT license
   <LICENSE-MIT> or
   <http://opensource.org/licenses/MIT>,

at your option.
````

### Notice 61

````text
Short version for non-lawyers:

`cap-primitives` is triple-licensed under Apache 2.0 with the LLVM Exception,
Apache 2.0, and MIT terms.


Longer version:

Copyrights in the `cap-primitives` project are retained by their contributors.
No copyright assignment is required to contribute to the `cap-primitives`
project.

Some files include code derived from Rust's `libstd`; see the comments in
the code for details.

Except as otherwise noted (below and/or in individual files), `cap-primitives`
is licensed under:

 - the Apache License, Version 2.0, with the LLVM Exception
   <LICENSE-Apache-2.0_WITH_LLVM-exception> or
   <http://llvm.org/foundation/relicensing/LICENSE.txt>
 - the Apache License, Version 2.0
   <LICENSE-APACHE> or
   <http://www.apache.org/licenses/LICENSE-2.0>,
 - or the MIT license
   <LICENSE-MIT> or
   <http://opensource.org/licenses/MIT>,

at your option.
````

### Notice 62

````text
Short version for non-lawyers:

`cap-std` is triple-licensed under Apache 2.0 with the LLVM Exception,
Apache 2.0, and MIT terms.


Longer version:

Copyrights in the `cap-std` project are retained by their contributors.
No copyright assignment is required to contribute to the `cap-std`
project.

Some files include code derived from Rust's `libstd`; see the comments in
the code for details.

Except as otherwise noted (below and/or in individual files), `cap-std`
is licensed under:

 - the Apache License, Version 2.0, with the LLVM Exception
   <LICENSE-Apache-2.0_WITH_LLVM-exception> or
   <http://llvm.org/foundation/relicensing/LICENSE.txt>
 - the Apache License, Version 2.0
   <LICENSE-APACHE> or
   <http://www.apache.org/licenses/LICENSE-2.0>,
 - or the MIT license
   <LICENSE-MIT> or
   <http://opensource.org/licenses/MIT>,

at your option.
````

### Notice 63

````text
Short version for non-lawyers:

`fs-set-times` is triple-licensed under Apache 2.0 with the LLVM Exception,
Apache 2.0, and MIT terms.


Longer version:

Copyrights in the `fs-set-times` project are retained by their contributors.
No copyright assignment is required to contribute to the `fs-set-times`
project.

Some files include code derived from Rust's `libstd`; see the comments in
the code for details.

Except as otherwise noted (below and/or in individual files), `fs-set-times`
is licensed under:

 - the Apache License, Version 2.0, with the LLVM Exception
   <LICENSE-Apache-2.0_WITH_LLVM-exception> or
   <http://llvm.org/foundation/relicensing/LICENSE.txt>
 - the Apache License, Version 2.0
   <LICENSE-APACHE> or
   <http://www.apache.org/licenses/LICENSE-2.0>,
 - or the MIT license
   <LICENSE-MIT> or
   <http://opensource.org/licenses/MIT>,

at your option.
````

### Notice 64

````text
Short version for non-lawyers:

`io-extras` is triple-licensed under Apache 2.0 with the LLVM Exception,
Apache 2.0, and MIT terms.


Longer version:

Copyrights in the `io-extras` project are retained by their contributors.
No copyright assignment is required to contribute to the `io-extras`
project.

Some files include code derived from Rust's `libstd`; see the comments in
the code for details.

Except as otherwise noted (below and/or in individual files), `io-extras`
is licensed under:

 - the Apache License, Version 2.0, with the LLVM Exception
   <LICENSE-Apache-2.0_WITH_LLVM-exception> or
   <http://llvm.org/foundation/relicensing/LICENSE.txt>
 - the Apache License, Version 2.0
   <LICENSE-APACHE> or
   <http://www.apache.org/licenses/LICENSE-2.0>,
 - or the MIT license
   <LICENSE-MIT> or
   <http://opensource.org/licenses/MIT>,

at your option.
````

### Notice 65

````text
Short version for non-lawyers:

`io-lifetimes` is triple-licensed under Apache 2.0 with the LLVM Exception,
Apache 2.0, and MIT terms.


Longer version:

Copyrights in the `io-lifetimes` project are retained by their contributors.
No copyright assignment is required to contribute to the `io-lifetimes`
project.

Some files include code derived from Rust's `libstd`; see the comments in
the code for details.

Except as otherwise noted (below and/or in individual files), `io-lifetimes`
is licensed under:

 - the Apache License, Version 2.0, with the LLVM Exception
   <LICENSE-Apache-2.0_WITH_LLVM-exception> or
   <http://llvm.org/foundation/relicensing/LICENSE.txt>
 - the Apache License, Version 2.0
   <LICENSE-APACHE> or
   <http://www.apache.org/licenses/LICENSE-2.0>,
 - or the MIT license
   <LICENSE-MIT> or
   <http://opensource.org/licenses/MIT>,

at your option.
````

### Notice 66

````text
Copyright 2017 Juniper Networks, Inc.

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
````

### Notice 67

````text
Copyright (c) 2016 Philipp Korber

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
````

### Notice 68

````text
Short version for non-lawyers:

`rustix-linux-procfs` is triple-licensed under Apache 2.0 with the LLVM Exception,
Apache 2.0, and MIT terms.


Longer version:

Copyrights in the `rustix-linux-procfs` project are retained by their contributors.
No copyright assignment is required to contribute to the `rustix-linux-procfs`
project.

Some files include code derived from Rust's `libstd`; see the comments in
the code for details.

Except as otherwise noted (below and/or in individual files), `rustix-linux-procfs`
is licensed under:

 - the Apache License, Version 2.0, with the LLVM Exception
   <LICENSE-Apache-2.0_WITH_LLVM-exception> or
   <http://llvm.org/foundation/relicensing/LICENSE.txt>
 - the Apache License, Version 2.0
   <LICENSE-APACHE> or
   <http://www.apache.org/licenses/LICENSE-2.0>,
 - or the MIT license
   <LICENSE-MIT> or
   <http://opensource.org/licenses/MIT>,

at your option.
````

### Notice 69

````text
Apache License
                           Version 2.0, January 2004
                        http://www.apache.org/licenses/

   TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

   1. Definitions.

      "License" shall mean the terms and conditions for use, reproduction,
      and distribution as defined by Sections 1 through 9 of this document.

      "Licensor" shall mean the copyright owner or entity authorized by
      the copyright owner that is granting the License.

      "Legal Entity" shall mean the union of the acting entity and all
      other entities that control, are controlled by, or are under common
      control with that entity. For the purposes of this definition,
      "control" means (i) the power, direct or indirect, to cause the
      direction or management of such entity, whether by contract or
      otherwise, or (ii) ownership of fifty percent (50%) or more of the
      outstanding shares, or (iii) beneficial ownership of such entity.

      "You" (or "Your") shall mean an individual or Legal Entity
      exercising permissions granted by this License.

      "Source" form shall mean the preferred form for making modifications,
      including but not limited to software source code, documentation
      source, and configuration files.

      "Object" form shall mean any form resulting from mechanical
      transformation or translation of a Source form, including but
      not limited to compiled object code, generated documentation,
      and conversions to other media types.

      "Work" shall mean the work of authorship, whether in Source or
      Object form, made available under the License, as indicated by a
      copyright notice that is included in or attached to the work
      (an example is provided in the Appendix below).

      "Derivative Works" shall mean any work, whether in Source or Object
      form, that is based on (or derived from) the Work and for which the
      editorial revisions, annotations, elaborations, or other modifications
      represent, as a whole, an original work of authorship. For the purposes
      of this License, Derivative Works shall not include works that remain
      separable from, or merely link (or bind by name) to the interfaces of,
      the Work and Derivative Works thereof.

      "Contribution" shall mean any work of authorship, including
      the original version of the Work and any modifications or additions
      to that Work or Derivative Works thereof, that is intentionally
      submitted to Licensor for inclusion in the Work by the copyright owner
      or by an individual or Legal Entity authorized to submit on behalf of
      the copyright owner. For the purposes of this definition, "submitted"
      means any form of electronic, verbal, or written communication sent
      to the Licensor or its representatives, including but not limited to
      communication on electronic mailing lists, source code control systems,
      and issue tracking systems that are managed by, or on behalf of, the
      Licensor for the purpose of discussing and improving the Work, but
      excluding communication that is conspicuously marked or otherwise
      designated in writing by the copyright owner as "Not a Contribution."

      "Contributor" shall mean Licensor and any individual or Legal Entity
      on behalf of whom a Contribution has been received by Licensor and
      subsequently incorporated within the Work.

   2. Grant of Copyright License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      copyright license to reproduce, prepare Derivative Works of,
      publicly display, publicly perform, sublicense, and distribute the
      Work and such Derivative Works in Source or Object form.

   3. Grant of Patent License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      (except as stated in this section) patent license to make, have made,
      use, offer to sell, sell, import, and otherwise transfer the Work,
      where such license applies only to those patent claims licensable
      by such Contributor that are necessarily infringed by their
      Contribution(s) alone or by combination of their Contribution(s)
      with the Work to which such Contribution(s) was submitted. If You
      institute patent litigation against any entity (including a
      cross-claim or counterclaim in a lawsuit) alleging that the Work
      or a Contribution incorporated within the Work constitutes direct
      or contributory patent infringement, then any patent licenses
      granted to You under this License for that Work shall terminate
      as of the date such litigation is filed.

   4. Redistribution. You may reproduce and distribute copies of the
      Work or Derivative Works thereof in any medium, with or without
      modifications, and in Source or Object form, provided that You
      meet the following conditions:

      (a) You must give any other recipients of the Work or
          Derivative Works a copy of this License; and

      (b) You must cause any modified files to carry prominent notices
          stating that You changed the files; and

      (c) You must retain, in the Source form of any Derivative Works
          that You distribute, all copyright, patent, trademark, and
          attribution notices from the Source form of the Work,
          excluding those notices that do not pertain to any part of
          the Derivative Works; and

      (d) If the Work includes a "NOTICE" text file as part of its
          distribution, then any Derivative Works that You distribute must
          include a readable copy of the attribution notices contained
          within such NOTICE file, excluding those notices that do not
          pertain to any part of the Derivative Works, in at least one
          of the following places: within a NOTICE text file distributed
          as part of the Derivative Works; within the Source form or
          documentation, if provided along with the Derivative Works; or,
          within a display generated by the Derivative Works, if and
          wherever such third-party notices normally appear. The contents
          of the NOTICE file are for informational purposes only and
          do not modify the License. You may add Your own attribution
          notices within Derivative Works that You distribute, alongside
          or as an addendum to the NOTICE text from the Work, provided
          that such additional attribution notices cannot be construed
          as modifying the License.

      You may add Your own copyright statement to Your modifications and
      may provide additional or different license terms and conditions
      for use, reproduction, or distribution of Your modifications, or
      for any such Derivative Works as a whole, provided Your use,
      reproduction, and distribution of the Work otherwise complies with
      the conditions stated in this License.

   5. Submission of Contributions. Unless You explicitly state otherwise,
      any Contribution intentionally submitted for inclusion in the Work
      by You to the Licensor shall be under the terms and conditions of
      this License, without any additional terms or conditions.
      Notwithstanding the above, nothing herein shall supersede or modify
      the terms of any separate license agreement you may have executed
      with Licensor regarding such Contributions.

   6. Trademarks. This License does not grant permission to use the trade
      names, trademarks, service marks, or product names of the Licensor,
      except as required for reasonable and customary use in describing the
      origin of the Work and reproducing the content of the NOTICE file.

   7. Disclaimer of Warranty. Unless required by applicable law or
      agreed to in writing, Licensor provides the Work (and each
      Contributor provides its Contributions) on an "AS IS" BASIS,
      WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
      implied, including, without limitation, any warranties or conditions
      of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
      PARTICULAR PURPOSE. You are solely responsible for determining the
      appropriateness of using or redistributing the Work and assume any
      risks associated with Your exercise of permissions under this License.

   8. Limitation of Liability. In no event and under no legal theory,
      whether in tort (including negligence), contract, or otherwise,
      unless required by applicable law (such as deliberate and grossly
      negligent acts) or agreed to in writing, shall any Contributor be
      liable to You for damages, including any direct, indirect, special,
      incidental, or consequential damages of any character arising as a
      result of this License or out of the use or inability to use the
      Work (including but not limited to damages for loss of goodwill,
      work stoppage, computer failure or malfunction, or any and all
      other commercial damages or losses), even if such Contributor
      has been advised of the possibility of such damages.

   9. Accepting Warranty or Additional Liability. While redistributing
      the Work or Derivative Works thereof, You may choose to offer,
      and charge a fee for, acceptance of support, warranty, indemnity,
      or other liability obligations and/or rights consistent with this
      License. However, in accepting such obligations, You may act only
      on Your own behalf and on Your sole responsibility, not on behalf
      of any other Contributor, and only if You agree to indemnify,
      defend, and hold each Contributor harmless for any liability
      incurred by, or claims asserted against, such Contributor by reason
      of your accepting any such warranty or additional liability.

   END OF TERMS AND CONDITIONS

   APPENDIX: How to apply the Apache License to your work.

      To apply the Apache License to your work, attach the following
      boilerplate notice, with the fields enclosed by brackets "[]"
      replaced with your own identifying information. (Don't include
      the brackets!)  The text should be enclosed in the appropriate
      comment syntax for the file format. We also recommend that a
      file or class name and description of purpose be included on the
      same "printed page" as the copyright notice for easier
      identification within third-party archives.

   Copyright [yyyy] [name of copyright owner]

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

       http://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.


--- LLVM Exceptions to the Apache 2.0 License ----

As an exception, if, as a result of your compiling your source code, portions
of this Software are embedded into an Object form of such source code, you
may redistribute such embedded portions in such Object form without complying
with the conditions of Sections 4(a), 4(b) and 4(d) of the License.

In addition, if you combine or link compiled forms of this Software with
software that is licensed under the GPLv2 ("Combined Software") and if a
court of competent jurisdiction determines that the patent provision (Section
3), the indemnity provision (Section 9) or other Section of the License
conflicts with the conditions of the GPLv2, you may retroactively and
prospectively choose to deem waived or otherwise exclude such Section(s) of
the License, but only in their entirety and only with respect to the Combined
Software.
````

### Notice 70

````text
The MIT License

Copyright 2015 Google Inc. All rights reserved.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
````

### Notice 71

````text
Copyright (c) 2014-2026 Sean McArthur

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
````
