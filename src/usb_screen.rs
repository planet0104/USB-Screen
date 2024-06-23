use futures_lite::future::block_on;
use image::{Rgb, RgbImage};
use nusb::Interface;
use anyhow::{anyhow, Result};
use serialport::{SerialPort, SerialPortInfo, SerialPortType};

use crate::rgb565::rgb888_to_rgb565_be;

const BULK_OUT_EP: u8 = 0x01;
const BULK_IN_EP: u8 = 0x81;

#[derive(Clone)]
pub struct UsbScreenInfo{
    pub label: String,
    pub address: String,
    pub width: u16,
    pub height: u16,
}

pub enum UsbScreen{
    USBRaw((UsbScreenInfo, Interface)),
    USBSerail((UsbScreenInfo, Box<dyn SerialPort>))
}

impl UsbScreen{
    pub fn draw_rgb_image(&mut self, x: u16, y: u16, img:&RgbImage) -> anyhow::Result<()>{
        //如果图像比屏幕大， 不绘制，否则会RP2040死机导致卡住
        match self{
            UsbScreen::USBRaw((info, interface)) => {
                if img.width() <= info.width as u32 && img.height() <= info.height as u32{
                    let _ = draw_rgb_image(x, y, img, interface);
                }
            }

            UsbScreen::USBSerail((info, port)) => {
                if img.width() <= info.width as u32 && img.height() <= info.height as u32{
                    let _ = draw_rgb_image_serial(x, y, img, port.as_mut());
                }
            }
        }
        Ok(())
    }

    pub fn open(info: UsbScreenInfo) -> Result<Self>{
        println!("打开屏幕:label={} addr={} {}x{}", info.label, info.address, info.width, info.height);
        let addr = info.address.clone();
        if info.label.contains("Screen"){
            //USB Raw设备, addr是device_address
            Ok(Self::USBRaw((info, open_usb_raw_device(&addr)?)))
        }else{
            //USB串口设备, addr是串口名称
            let screen = serialport::new(&info.address, 115_200).open()?;
            Ok(Self::USBSerail((info, screen)))
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

// 查询所有USB屏幕设备
// 对于USB Raw返回的第2个参数是 device_address
// 对于USB Serial, 返回的第2个参数是串口名称
pub fn find_all_device() -> Vec<UsbScreenInfo>{
    let mut devices = vec![];
    if let Ok(di) = nusb::list_devices(){
        for d in di{
            let serial_number = d.serial_number().unwrap_or("");
            if  d.product_string().unwrap_or("") == "USB Screen" && serial_number.starts_with("USBSCR"){
                let label = format!("USB Screen({})", d.device_address());
                let address = format!("{}", d.device_address());
                //从串号中读取屏幕大小
                let screen_size = &serial_number[6..serial_number.find(";").unwrap_or(13)];
                let screen_size = screen_size.replace("X", "x");
                let mut arr = screen_size.split("x");
                let width = arr.next().unwrap_or("160").parse::<u16>().unwrap_or(160);
                let height = arr.next().unwrap_or("128").parse::<u16>().unwrap_or(128);
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
    if let Ok(serial_devices) = find_usb_serial_device(){
        // println!("USB Serial 设备数量:{}", serial_devices.len());
        for (dev, serial_number) in serial_devices{
            let label = format!("USB {}", dev.port_name);
            let address = format!("{}", dev.port_name);
            //从串号中读取屏幕大小
            let screen_size = &serial_number[6..serial_number.find(";").unwrap_or(13)].to_string();
            let screen_size = screen_size.replace("X", "x");
            let mut arr = screen_size.split("x");
            let width = arr.next().unwrap_or("160").parse::<u16>().unwrap_or(160);
            let height = arr.next().unwrap_or("128").parse::<u16>().unwrap_or(128);
            devices.push(UsbScreenInfo { label, address, width, height });
        }
    }
    // println!("usb 设备:{:?}", devices);
    devices
}

pub fn find_usb_serial_device() -> Result<Vec<(SerialPortInfo, String)>>{
    let ports: Vec<SerialPortInfo> = serialport::available_ports().unwrap_or(vec![]);
    let mut usb_screen = vec![];
    for p in ports {
        match p.port_type.clone(){
            SerialPortType::UsbPort(port) => {
                let serial_number = port.serial_number.unwrap_or("".to_string());
                if serial_number.starts_with("USBSCR"){
                    usb_screen.push((p, serial_number));
                    continue;
                }
            }
            _ => ()
        }
    }
    Ok(usb_screen)
}

pub fn clear_screen(color: Rgb<u8>, interface:&Interface, width: u16, height: u16) -> anyhow::Result<()>{
    let mut img = RgbImage::new(width as u32, height as u32);
    for p in img.pixels_mut(){
        *p = color;
    }
    draw_rgb_image(0, 0, &img, interface)
}

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
    // println!("draw:{x}x{y} {width}x{height}");

    block_on(interface.bulk_out(BULK_OUT_EP, img_begin.into())).status?;
    //读取
    // let result = block_on(interface.bulk_in(BULK_IN_EP, RequestBuffer::new(64))).data;
    // let msg = String::from_utf8(result)?;
    // println!("{msg}ms");

    block_on(interface.bulk_out(BULK_OUT_EP, rgb565_u8_slice.into())).status?;
    block_on(interface.bulk_out(BULK_OUT_EP, IMAGE_BB.to_be_bytes().into())).status?;
    Ok(())
}

pub fn draw_rgb_image_serial(x: u16, y: u16, img:&RgbImage, port:&mut dyn SerialPort) -> anyhow::Result<()>{
    //ST7789驱动使用的是Big-Endian
    let rgb565 = rgb888_to_rgb565_be(&img, img.width() as usize, img.height() as usize);
    draw_rgb565_serial(&rgb565, x, y, img.width() as u16, img.height() as u16, port)
}

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