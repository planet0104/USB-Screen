# 必须在linux/wsl2 linux中执行编译
# 二进制大小: 6.38M
#
# arm64版本不支持编辑器，修改默认features后再编译：
# [features]
# default = ["v4l-webcam"]
#
# cargo install cross --git https://github.com/cross-rs/cross
cross build --target aarch64-unknown-linux-gnu --release