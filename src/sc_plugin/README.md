# SuperStecker

## Configure

```shell
SC_SRC_PATH=/path/to/sc/source cmake --preset macOS
```

## Build

```shell
cmake --build build
```

## Install

Build and push files into `install` folder which can be linked from `Platform.userExtensionDir`.

```shell
cmake --build build --target install
```

> Also possible to build a release build via
>
> ```shell
> cmake --build build --target install --config Release
> ```
>
