# USB Screen
 USB屏幕&编辑器


## windows
```cmd
./run.cmd
```

## Ubuntu

```bash
# export https_proxy=http://192.168.1.25:6003;export http_proxy=http://192.168.1.25:6003;export all_proxy=socks5://192.168.1.25:6003
# export https_proxy=;export http_proxy=;export all_proxy=;
sudo apt-get install -y libv4l-dev
sudo apt-get install libclang-dev
# sudo apt install -y clang libavcodec-dev libavformat-dev libavfilter-dev libavdevice-dev libavutil-dev pkg-config
# sudo apt install yasm

sh run.sh
# sudo ./target/debug/USB-Screen
# sudo ./target/debug/USB-Screen editor

## v4l utils
## sudo apt install v4l-utils
## v4l2-ctl  --list-formats -d /dev/video0
## v4l2-ctl --list-formats-ext -d /dev/video0
```