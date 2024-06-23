use image::RgbImage;

/// A 16bit pixel that has 5 red bits, 6 green bits and  5 blue bits
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct Rgb565Pixel(pub u16);

impl Rgb565Pixel {
    const R_MASK: u16 = 0b1111_1000_0000_0000;
    const G_MASK: u16 = 0b0000_0111_1110_0000;
    const B_MASK: u16 = 0b0000_0000_0001_1111;

    /// Return the red component as a u8.
    ///
    /// The bits are shifted so that the result is between 0 and 255
    fn red(self) -> u8 {
        ((self.0 & Self::R_MASK) >> 8) as u8
    }
    /// Return the green component as a u8.
    ///
    /// The bits are shifted so that the result is between 0 and 255
    fn green(self) -> u8 {
        ((self.0 & Self::G_MASK) >> 3) as u8
    }
    /// Return the blue component as a u8.
    ///
    /// The bits are shifted so that the result is between 0 and 255
    fn blue(self) -> u8 {
        ((self.0 & Self::B_MASK) << 3) as u8
    }
}

impl Rgb565Pixel{
    pub fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self(((r as u16 & 0b11111000) << 8) | ((g as u16 & 0b11111100) << 3) | (b as u16 >> 3))
    }
}

// pub fn rgb888_to_rgb565_u16(img: &[u8], width: usize, height: usize) -> Vec<u16>{
//     let mut rgb565 = vec![0u16; width * height];
//     for (i, p) in img.chunks(3).enumerate(){
//         let rgb565_pixel: Rgb565Pixel = Rgb565Pixel::from_rgb(p[0], p[1], p[2]);
//         rgb565[i] = rgb565_pixel.0;
//     }
//     rgb565
// }

pub fn rgb888_to_rgb565_be(img: &[u8], width: usize, height: usize) -> Vec<u8>{
    let mut rgb565 = Vec::with_capacity(width * height * 2);
    for p in img.chunks(3){
        let rgb565_pixel: Rgb565Pixel = Rgb565Pixel::from_rgb(p[0], p[1], p[2]);
        let be_bytes = rgb565_pixel.0.to_be_bytes();
        rgb565.push(be_bytes[0]);
        rgb565.push(be_bytes[1]);
    }
    rgb565
}

pub fn rgb888_to_rgb565_le(img: &[u8], width: usize, height: usize) -> Vec<u8>{
    let mut rgb565 = Vec::with_capacity(width * height * 2);
    for p in img.chunks(3){
        let rgb565_pixel: Rgb565Pixel = Rgb565Pixel::from_rgb(p[0], p[1], p[2]);
        let be_bytes = rgb565_pixel.0.to_le_bytes();
        rgb565.push(be_bytes[0]);
        rgb565.push(be_bytes[1]);
    }
    rgb565
}

pub fn rgb565_u16_image_to_rgb(rgb565: &[u16], width: u32, height: u32) -> RgbImage{
    let mut rgb = RgbImage::new(width, height);
    for (i, p) in rgb.pixels_mut().enumerate(){
        let rgb565_pixel = Rgb565Pixel(rgb565[i]);
        p[0] = rgb565_pixel.red();
        p[1] = rgb565_pixel.green();
        p[2] = rgb565_pixel.blue();
    }
    rgb
}

pub fn rgb565_image_to_rgb(rgb565: &[u8], width: u32, height: u32) -> RgbImage{
    let mut rgb = RgbImage::new(width, height);
    for (i, p) in rgb.pixels_mut().enumerate(){
        let be_bytes = &rgb565[i*2..i*2+2];
        let rgb565_pixel = u16::from_le_bytes([be_bytes[0], be_bytes[1]]);
        let rgb565_pixel = Rgb565Pixel(rgb565_pixel);
        p[0] = rgb565_pixel.red();
        p[1] = rgb565_pixel.green();
        p[2] = rgb565_pixel.blue();
    }
    rgb
}