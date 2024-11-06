#[inline]
pub fn rgb_to_rgb565(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 & 0b11111000) << 8) | ((g as u16 & 0b11111100) << 3) | (b as u16 >> 3)
}

pub fn rgb888_to_rgb565_be(img: &[u8], width: usize, height: usize) -> Vec<u8>{
    let mut rgb565 = Vec::with_capacity(width * height * 2);
    for p in img.chunks(3){
        let rgb565_pixel = rgb_to_rgb565(p[0], p[1], p[2]);
        rgb565.extend_from_slice(&rgb565_pixel.to_be_bytes());
    }
    rgb565
}