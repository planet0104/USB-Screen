use std::mem;

use anyhow::{anyhow, Result};
use futures_lite::future::block_on;
use image::RgbImage;
use nusb::Interface;

use crate::rgb565::{rgb888_to_rgb565_u16, Rgb565Pixel};

const BULK_OUT_EP: u8 = 0x01;
const BULK_IN_EP: u8 = 0x81;

pub const SCREEN_WIDTH: u16 = 160;
pub const SCREEN_HEIGHT: u16 = 128;

pub fn open_usb_screen(product_string: &str, serial_number: &str) -> Result<Option<Interface>> {
    let mut di = nusb::list_devices()?;
    match di.find(|d| {
        d.product_string() == Some(product_string) && d.serial_number() == Some(serial_number)
    }) {
        Some(di) => {
            let device = di.open()?;
            let interface = device.claim_interface(0)?;
            Ok(Some(interface))
        }
        None => Ok(None),
    }
}

pub fn clear_screen(color: Rgb565Pixel, interface: &Interface) -> anyhow::Result<()> {
    let pixels = vec![color.0; (SCREEN_WIDTH * SCREEN_HEIGHT) as usize];
    draw_rgb565(&pixels, 0, 0, SCREEN_WIDTH, SCREEN_HEIGHT, interface)
}

pub fn draw_rgb_image(x: u16, y: u16, img: &RgbImage, interface: &Interface) -> anyhow::Result<()> {
    let rgb565: Vec<u16> = rgb888_to_rgb565_u16(&img, img.width() as usize, img.height() as usize);
    draw_rgb565(
        &rgb565,
        x,
        y,
        img.width() as u16,
        img.height() as u16,
        interface,
    )
}

pub fn draw_rgb565(
    rgb565: &[u16],
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    interface: &Interface,
) -> anyhow::Result<()> {
    let rgb565_byte_count = rgb565.len() * mem::size_of::<u16>();
    let rgb565_u16_ptr: *const u16 = rgb565.as_ptr();
    let rgb565_u8_ptr: *const u8 = rgb565_u16_ptr as *const u8;
    let rgb565_u8_slice: &[u8] =
        unsafe { std::slice::from_raw_parts(rgb565_u8_ptr, rgb565_byte_count) };

    const IMAGE_AA: u64 = 7596835243154170209;
    const BOOT_USB: u64 = 7093010483740242786;
    const IMAGE_BB: u64 = 7596835243154170466;

    let img_begin = &mut [0u8; 16];
    img_begin[0..8].copy_from_slice(&IMAGE_AA.to_be_bytes());
    img_begin[8..10].copy_from_slice(&width.to_be_bytes());
    img_begin[10..12].copy_from_slice(&height.to_be_bytes());
    img_begin[12..14].copy_from_slice(&x.to_be_bytes());
    img_begin[14..16].copy_from_slice(&y.to_be_bytes());

    let e = block_on(interface.bulk_out(BULK_OUT_EP, img_begin.into()));
    if e.status.is_err() {
        return Err(anyhow!("{:?}", e.status.err()));
    }
    let _ = block_on(interface.bulk_out(BULK_OUT_EP, rgb565_u8_slice.into()));
    let _ = block_on(interface.bulk_out(BULK_OUT_EP, IMAGE_BB.to_be_bytes().into()));
    Ok(())
}
