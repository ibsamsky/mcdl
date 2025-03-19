# mcdl

## What is this?

`mcdl` is a project that I started in March 2023 in order to learn Rust and provide myself with an easier way to manage and host Minecraft servers. Most notably, it makes use of async I/O and multithreading with `tokio`, detailed error handling and logging with `color-eyre` and `tracing`, parsing and deserialization from web APIs with `serde`, and shared metadata state that is passed between threads.

Current features include:

- Listing and filtering Minecraft versions from Mojang's API
- Installation, management, and launching of Minecraft server instances
  - The correct Java runtime for each version is chosen and downloaded automatically
  - Provides an option to upload crash reports to a third-party pastebin service (mclo.gs)
- Configuration file support for command-line flags passed to the server
  - Generated automatically upon installation and can be edited manually

Please note that this project was started when I was relatively new to Rust, so many of the design decisions and implementations are not optimal/idiomatic. I am in the process of rewriting it to be more in line with best practices.[^1]

[^1]: Finding the source of this rewrite is left as an exercise for the reader. Once it's mostly complete, it will be merged into this repository.

## Installation

### Compiled binaries

Binaries for Linux (amd64 and aarch64), Windows (amd64), and macOS (aarch64, experimental) are available as [Actions artifacts][actions]
or from [nightly.link][nightly] if you're not logged in. If you're not sure which one to use,
try `linux.nightly.release` for Linux and `windows-msvc.nightly.release` for Windows.

### From source

```sh
cargo install --git https://github.com/ibsamsky/mcdl
```

[actions]: https://github.com/ibsamsky/mcdl/actions?query=is%3Asuccess+workflow%3Aci
[nightly]: https://nightly.link/ibsamsky/mcdl/workflows/test/main

## Todo (rough)

- [ ] types/meta
  - [ ] `Settings` struct
    - [ ] configure certain paths, i.e. instance dir
    - [ ] global default java flags (maybe)
- [ ] main
  - [ ] alternative outputs (JSON/debug/etc.) for info/list commands
  - [ ] third-party servers (fabric, forge, etc.)
  - [ ] instance id separate from version/multi-instance for same version
    - [ ] install-multiple support, e.g. `mcdl install -v 1.17.1 -i fabric -v 1.17.1 -i forge` or `mcdl install -v 1.17.1:forge -v 1.17.1:fabric`
- [ ] types/version
  - [ ] fabric meta (?)

---

By using this software, you agree to the [Minecraft EULA][eula]. This software is not affiliated with Mojang Studios or Microsoft.

[eula]: https://www.minecraft.net/en-us/eula
