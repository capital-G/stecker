# SuperStecker

SuperStecker is a set of SuperCollider UGens which allow to connect to a Stecker server, allowing to send and receive audio and control signals.

## Build

Building requires Rust to be installed.

Configure a build by executing the following command, where the presets can be listed via `cmake --list-presets` and `SC_SRC_PATH` should point to the directory of a local copy of the SuperCollider source files.

```shell
SC_SRC_PATH=/path/to/sc/source cmake --preset macOS
```

To build, execute

```shell
cmake --build build
```

To build an `install` bundle which can be copied/linked into the Extension folder located at `Platform.userExtensionDir` use

```shell
cmake --build build --target install
```

Release builds can be done by adding the flag `--config Release`.
