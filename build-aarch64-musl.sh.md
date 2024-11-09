# 必须在linux/wsl2 linux中执行编译
#
# 修改默认features后再编译：
# [features]
# default = ["v4l-webcam", "usb-serial"]
#
# cargo install cross --git https://github.com/cross-rs/cross

# 启动docker后运行
# 复制出来再运行
bash
cross build --target aarch64-unknown-linux-musl --release

# openwrp配置花生壳教程
https://service.oray.com/question/20547.html

# 开机运行执行：
ln -s /etc/init.d/mystart /etc/rc.d/S99mystart
#ln -s /etc/init.d/mystart /etc/rc.d/K15mystart
# 查看启动日志
logread > log.txt

# openwrt防火墙设置(网络->防火墙)

https://www.bilibili.com/read/cv12684340/

# openwrt配置usb设备

https://openwrt.org/docs/guide-user/storage/usb-installing

```shell
opkg update
echo host > /sys/kernel/debug/usb/ci_hdrc.0/role
opkg install kmod-usb-net kmod-usb-net-rndis kmod-usb-net-cdc-ether usbutils 
lsusb
#=====================

#获取已安装的 USB 软件包列表
opkg list-installed *usb*
#安装 USB 核心包（所有 USB 版本），如果前面的 list-output 未列出它
opkg install kmod-usb-core
insmod usbcore
#安装 USB 存储包（所有 USB 版本），如果前面的 list-output 未列出它
opkg install kmod-usb-storage
#要安装 USB 1.1 驱动程序，请先尝试 UHCI 驱动程序
opkg install kmod-usb-uhci
insmod uhci_hcd
#如果此操作失败并显示错误“No such device”，请尝试安装 USB 1.1 的替代 OHCI 驱动程序

```