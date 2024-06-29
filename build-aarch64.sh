# 必须在linux/wsl2 linux中执行编译
# 二进制大小: 6.38M
#
# arm64版本不支持编辑器，修改默认features后再编译：
# [features]
# default = ["v4l-webcam"]
#
# cargo install cross --git https://github.com/cross-rs/cross
# cross build --target aarch64-unknown-linux-gnu --release

# 复制出来再运行
RUSTFLAGS="-Zlocation-detail=none" cross +nightly build -Z build-std=std,panic_abort \
  -Z build-std-features=panic_immediate_abort \
  -Z build-std-features="optimize_for_size" \
  --target aarch64-unknown-linux-gnu --release