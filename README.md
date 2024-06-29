# USB Screen
 USB屏幕&编辑器

# 编译

## 编译aarch64-linux

1、设置default features，只启用 v4l-webcam

```toml
[features]
default = ["v4l-webcam"]
```

2、启动 DockerDesktop

3、进入 wsl2 Ubuntu

4、安装 cross

```shell
cargo install cross --git https://github.com/cross-rs/cross
```

5、编译

注意 Cross.toml 中的配置

```shell
# rustup component add rust-src --toolchain nightly
RUSTFLAGS="-Zlocation-detail=none" cross +nightly build -Z build-std=std,panic_abort \
  -Z build-std-features=panic_immediate_abort \
  -Z build-std-features="optimize_for_size" \
  --target aarch64-unknown-linux-gnu --release
```

# 运行编辑器

## windows中运行

设置 deault features

```toml
[features]
default = ["editor", "tray", "nokhwa-webcam"]
```

```cmd
./run.cmd
```

## Ubuntu中运行

设置 deault features

```toml
[features]
default = ["editor", "v4l-webcam"]
```

```bash
# export https_proxy=http://192.168.1.25:6003;export http_proxy=http://192.168.1.25:6003;export all_proxy=socks5://192.168.1.25:6003
# export https_proxy=;export http_proxy=;export all_proxy=;
sudo apt-get install -y libclang-dev libv4l-dev libudev-dev

sh run.sh
# sudo ./target/debug/USB-Screen
# sudo ./target/debug/USB-Screen editor

## v4l utils
## sudo apt install v4l-utils
## v4l2-ctl  --list-formats -d /dev/video0
## v4l2-ctl --list-formats-ext -d /dev/video0
```