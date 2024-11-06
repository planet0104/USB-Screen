use std::time::Duration;

use futures_lite::future::block_on;
use image::{Rgb, RgbImage};
use log::{info, warn};
use nusb::Interface;
use anyhow::{anyhow, Result};
#[cfg(feature = "usb-serial")]
use serialport::{SerialPort, SerialPortInfo, SerialPortType};

use crate::rgb565::rgb888_to_rgb565_be;

// use crate::rgb565::rgb888_to_rgb565_be;

const BULK_OUT_EP: u8 = 0x01;
const BULK_IN_EP: u8 = 0x81;

#[derive(Clone, Debug)]
pub struct UsbScreenInfo{
    pub label: String,
    pub address: String,
    pub width: u16,
    pub height: u16,
}

pub enum UsbScreen{
    USBRaw((UsbScreenInfo, Interface)),
    #[cfg(feature = "usb-serial")]
    USBSerial((UsbScreenInfo, Box<dyn SerialPort>))
}

impl UsbScreen{
    pub fn draw_rgb_image(&mut self, x: u16, y: u16, img:&RgbImage) -> anyhow::Result<()>{
        //如果图像比屏幕大， 不绘制，否则会RP2040死机导致卡住
        match self{
            UsbScreen::USBRaw((info, interface)) => {
                if img.width() <= info.width as u32 && img.height() <= info.height as u32{
                    draw_rgb_image(x, y, img, interface)?;
                }
            }

            #[cfg(feature = "usb-serial")]
            UsbScreen::USBSerial((info, port)) => {
                if img.width() <= info.width as u32 && img.height() <= info.height as u32{
                    draw_rgb_image_serial(x, y, img, port.as_mut())?;
                }
            }
        }
        Ok(())
    }

    pub fn open(info: UsbScreenInfo) -> Result<Self>{
        info!("打开屏幕:label={} addr={} {}x{}", info.label, info.address, info.width, info.height);
        let addr = info.address.clone();
        if info.label.contains("Screen"){
            //USB Raw设备, addr是device_address
            Ok(Self::USBRaw((info, open_usb_raw_device(&addr)?)))
        }else{
            #[cfg(feature = "usb-serial")]
            {
                //USB串口设备, addr是串口名称
                let screen =  serialport::new(&info.address, 115_200).open()?;
                Ok(Self::USBSerial((info, screen)))
            }
            #[cfg(not(feature = "usb-serial"))]
            {
                Err(anyhow!("此平台不支持 USB串口设备"))
            }
        }
    }
}

pub fn find_and_open_a_screen() -> Option<UsbScreen>{
    //先查找串口设备
    let devices = find_all_device();
    for info in devices{
        if let Ok(screen) = UsbScreen::open(info){
            return Some(screen);
        }
    }
    None
}

pub fn open_usb_raw_device(device_address: &str) -> Result<Interface>{
    let di = nusb::list_devices()?;
    for d in di{
        if d.serial_number().unwrap_or("").starts_with("USBSCR") && d.device_address() == device_address.parse::<u8>()?{
            let device = d.open()?;
            let interface = device.claim_interface(0)?;
            return Ok(interface);
        }
    }
    Err(anyhow!("设备地址未找到"))
}

fn get_screen_size_from_serial_number(serial_number:&str) -> (u16, u16){
    //从串号中读取屏幕大小
    let screen_size = &serial_number[6..serial_number.find(";").unwrap_or(13)];
    let screen_size = screen_size.replace("X", "x");
    let mut arr = screen_size.split("x");
    let width = arr.next().unwrap_or("160").parse::<u16>().unwrap_or(160);
    let height = arr.next().unwrap_or("128").parse::<u16>().unwrap_or(128);
    (width, height)
}

// 查询所有USB屏幕设备
// 对于USB Raw返回的第2个参数是 device_address
// 对于USB Serial, 返回的第2个参数是串口名称
pub fn find_all_device() -> Vec<UsbScreenInfo>{
    let mut devices = vec![];
    if let Ok(di) = nusb::list_devices(){
        for d in di{
            #[cfg(not(windows))]
            info!("USB Raw设备:{:?}", d);
            let serial_number = d.serial_number().unwrap_or("");
            if  d.product_string().unwrap_or("") == "USB Screen" && serial_number.starts_with("USBSCR"){
                let label = format!("USB Screen({})", d.device_address());
                let address = format!("{}", d.device_address());
                let (width, height) = get_screen_size_from_serial_number(serial_number);
                devices.push(UsbScreenInfo{
                    label,
                    address,
                    width,
                    height,
                });
            }
        }
    }
    // println!("USB Raw设备数量:{}", devices.len());
    #[cfg(feature = "usb-serial")]
    devices.extend_from_slice(&find_usb_serial_device());
    #[cfg(not(windows))]
    info!("所有usb 设备:{:?}", devices);

    if devices.len() == 0{
        warn!("no available device!");
    }

    devices
}

#[cfg(feature = "usb-serial")]
pub fn find_usb_serial_device() -> Vec<UsbScreenInfo>{
    let ports: Vec<SerialPortInfo> = serialport::available_ports().unwrap_or(vec![]);
    let mut devices = vec![];
    for p in ports {
        #[cfg(not(windows))]
        info!("USB Serial 设备:{:?}", p);
        match p.port_type.clone(){
            SerialPortType::UsbPort(port) => {
                let serial_number = port.serial_number.unwrap_or("".to_string());
                if serial_number.starts_with("USBSCR"){
                    let port_name = p.port_name.clone();
                    let (width, height) = get_screen_size_from_serial_number(&serial_number);
                    devices.push(UsbScreenInfo{
                        label: format!("USB {port_name}"), address: port_name.to_string(),
                        width,
                        height,
                    });
                    continue;
                }
            }
            _ => ()
        }
    }
    devices
}

pub fn clear_screen(color: Rgb<u8>, interface:&Interface, width: u16, height: u16) -> anyhow::Result<()>{
    let mut img = RgbImage::new(width as u32, height as u32);
    for p in img.pixels_mut(){
        *p = color;
    }
    draw_rgb_image(0, 0, &img, interface)
}

#[cfg(feature = "usb-serial")]
pub fn clear_screen_serial(color: Rgb<u8>, port:&mut dyn SerialPort, width: u16, height: u16) -> anyhow::Result<()>{
    let mut img = RgbImage::new(width as u32, height as u32);
    for p in img.pixels_mut(){
        *p = color;
    }
    draw_rgb_image_serial(0, 0, &img, port)
}

pub fn draw_rgb_image(x: u16, y: u16, img:&RgbImage, interface:&Interface) -> anyhow::Result<()>{
    //ST7789驱动使用的是Big-Endian
    let rgb565 = rgb888_to_rgb565_be(&img, img.width() as usize, img.height() as usize);
    draw_rgb565(&rgb565, x, y, img.width() as u16, img.height() as u16, interface)
}

pub fn draw_rgb565(rgb565:&[u8], x: u16, y: u16, width: u16, height: u16, interface:&Interface) -> anyhow::Result<()>{
    // info!("压缩前大小:{}", rgb565.len());
    let rgb565_u8_slice = lz4_flex::compress_prepend_size(rgb565);
    // info!("压缩后大小:{}", rgb565_u8_slice.len());
    if rgb565_u8_slice.len() >1024*28 {
        return Err(anyhow!("图像太大了!"));
    }
    const IMAGE_AA:u64 = 7596835243154170209;
    const BOOT_USB:u64 = 7093010483740242786;
    const IMAGE_BB:u64 = 7596835243154170466;

    let img_begin = &mut [0u8; 16];
    img_begin[0..8].copy_from_slice(&IMAGE_AA.to_be_bytes());
    img_begin[8..10].copy_from_slice(&width.to_be_bytes());
    img_begin[10..12].copy_from_slice(&height.to_be_bytes());
    img_begin[12..14].copy_from_slice(&x.to_be_bytes());
    img_begin[14..16].copy_from_slice(&y.to_be_bytes());
    // info!("绘制:{x}x{y} {width}x{height}");
    // block_on(interface.bulk_out(BULK_OUT_EP, img_begin.into())).status?;
    block_on(async {
        async_std::future::timeout(Duration::from_millis(100), interface.bulk_out(BULK_OUT_EP, img_begin.into()))
            .await
    })?.status?;
    //读取
    // let result = block_on(interface.bulk_in(BULK_IN_EP, RequestBuffer::new(64))).data;
    // let msg = String::from_utf8(result)?;
    // println!("{msg}ms");
    // block_on(interface.bulk_out(BULK_OUT_EP, rgb565_u8_slice.into())).status?;
    block_on(async {
        async_std::future::timeout(Duration::from_millis(100), interface.bulk_out(BULK_OUT_EP, rgb565_u8_slice.into()))
            .await
    })?.status?;
    // block_on(interface.bulk_out(BULK_OUT_EP, IMAGE_BB.to_be_bytes().into())).status?;
    block_on(async {
        async_std::future::timeout(Duration::from_millis(100), interface.bulk_out(BULK_OUT_EP, IMAGE_BB.to_be_bytes().into()))
            .await
    })?.status?;
    // info!("绘制成功..");
    Ok(())
}

#[cfg(feature = "usb-serial")]
pub fn draw_rgb_image_serial(x: u16, y: u16, img:&RgbImage, port:&mut dyn SerialPort) -> anyhow::Result<()>{
    //ST7789驱动使用的是Big-Endian
    let rgb565 = rgb888_to_rgb565_be(&img, img.width() as usize, img.height() as usize);
    draw_rgb565_serial(&rgb565, x, y, img.width() as u16, img.height() as u16, port)
}

// 320x240屏幕连接到usb，然后在编辑器中一边添加多张gif，一边保存时，有时候rp2040会死机，同时编辑器也会卡死。
//第一：首先解决usb死机后，软件卡死问题
//第二：找到硬件代码死机问题，增加判断逻辑

#[cfg(feature = "usb-serial")]
pub fn draw_rgb565_serial(rgb565:&[u8], x: u16, y: u16, width: u16, height: u16, port:&mut dyn SerialPort) -> anyhow::Result<()>{
    
    let rgb565_u8_slice = lz4_flex::compress_prepend_size(rgb565);

    const IMAGE_AA:u64 = 7596835243154170209;
    const BOOT_USB:u64 = 7093010483740242786;
    const IMAGE_BB:u64 = 7596835243154170466;

    let img_begin = &mut [0u8; 16];
    img_begin[0..8].copy_from_slice(&IMAGE_AA.to_be_bytes());
    img_begin[8..10].copy_from_slice(&width.to_be_bytes());
    img_begin[10..12].copy_from_slice(&height.to_be_bytes());
    img_begin[12..14].copy_from_slice(&x.to_be_bytes());
    img_begin[14..16].copy_from_slice(&y.to_be_bytes());
    // println!("draw:{x}x{y} {width}x{height} len={}", rgb565_u8_slice.len());
    
    port.write(img_begin)?;
    port.flush()?;
    port.write(&rgb565_u8_slice)?;
    port.flush()?;
    port.write(&IMAGE_BB.to_be_bytes())?;
    port.flush()?;
    Ok(())
}

#[cfg(not(windows))]
fn list_acm_devices() -> Vec<String> {
    let dir_path = std::path::Path::new("/dev");
    let entries = match std::fs::read_dir(dir_path){
        Err(err) => {
            log::error!("error list /dev/ {:?}", err);
            return vec![];
        }
        Ok(e) => e
    };
    entries.filter_map(|entry| {
        entry.ok().and_then(|e| {
            let path = e.path();
            if let Some(file_name) = path.file_name() {
                if let Some(name) = file_name.to_str() {
                    if name.starts_with("ttyACM") {
                        return Some(format!("/dev/{name}"));
                    }
                }
            }
            None
        })
    }).collect()
}