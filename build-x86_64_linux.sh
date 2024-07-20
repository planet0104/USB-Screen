# 必须在linux/wsl2 linux中执行编译
# 二进制大小: 6.38M
#
# 修改默认features后再编译：
# [features]
# default = ["editor", "v4l-webcam", usb-serial]
#
# cargo install cross --git https://github.com/cross-rs/cross
# cross build --target x86_64-unknown-linux-gnu --release
bash
cargo build --target x86_64-unknown-linux-gnu --release

# https://github.com/johnthagen/min-sized-rust
# rustup component add rust-src --toolchain nightly
# RUSTFLAGS="-Zlocation-detail=none" cross +nightly build -Z build-std=std,panic_abort \
#   -Z build-std-features=panic_immediate_abort \
#   -Z build-std-features="optimize_for_size" \
#   --target x86_64-unknown-linux-gnu --release