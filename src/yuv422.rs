use anyhow::{anyhow, Result};
use v4l::FourCC;

pub const YUYV:FourCC = FourCC{repr: [89, 85, 89, 86] };
pub const RGB3:FourCC = FourCC{repr: [82, 71, 66, 51] };
pub const MJPG:FourCC = FourCC{repr: [77, 74, 80, 71] };

// For those maintaining this, I recommend you read: https://docs.microsoft.com/en-us/windows/win32/medfound/recommended-8-bit-yuv-formats-for-video-rendering#yuy2
// https://en.wikipedia.org/wiki/YUV#Converting_between_Y%E2%80%B2UV_and_RGB
// and this too: https://stackoverflow.com/questions/16107165/convert-from-yuv-420-to-imagebgr-byte
// The YUY2(YUYV) format is a 16 bit format. We read 4 bytes at a time to get 6 bytes of RGB888.
// First, the YUY2 is converted to YCbCr 4:4:4 (4:2:2 -> 4:4:4)
// then it is converted to 6 bytes (2 pixels) of RGB888
/// Converts a YUYV 4:2:2 datastream to a RGB888 Stream. [For further reading](https://en.wikipedia.org/wiki/YUV#Converting_between_Y%E2%80%B2UV_and_RGB)
/// # Errors
/// This may error when the data stream size is not divisible by 4, a i32 -> u8 conversion fails, or it fails to read from a certain index.
#[inline]
pub fn yuyv422_to_rgb(data: &[u8]) -> Result<Vec<u8>> {
    let pixel_size = 3;
    // yuyv yields 2 3-byte pixels per yuyv chunk
    let rgb_buf_size = (data.len() / 4) * (2 * pixel_size);

    let mut dest = vec![0; rgb_buf_size];
    buf_yuyv422_to_rgb(data, &mut dest)?;

    Ok(dest)
}

/// Same as [`yuyv422_to_rgb`] but with a destination buffer instead of a return `Vec<u8>`
/// # Errors
/// If the stream is invalid YUYV, or the destination buffer is not large enough, this will error.
#[inline]
pub fn buf_yuyv422_to_rgb(data: &[u8], dest: &mut [u8]) -> Result<()> {
    if data.len() % 4 != 0 {
        return Err(anyhow!("Assertion failure, the YUV stream isn't 4:2:2! (wrong number of bytes)"));
    }

    let pixel_size = 3;
    // yuyv yields 2 3-byte pixels per yuyv chunk
    let rgb_buf_size = (data.len() / 4) * (2 * pixel_size);

    if dest.len() != rgb_buf_size {
        return Err(anyhow!(format!("Assertion failure, the destination RGB buffer is of the wrong size! [expected: {rgb_buf_size}, actual: {}]", dest.len())));
    }

    let iter = data.chunks_exact(4);

    let mut iter = iter
        .flat_map(|yuyv| {
            let y1 = i32::from(yuyv[0]);
            let u = i32::from(yuyv[1]);
            let y2 = i32::from(yuyv[2]);
            let v = i32::from(yuyv[3]);
            let pixel1 = yuyv444_to_rgb(y1, u, v);
            let pixel2 = yuyv444_to_rgb(y2, u, v);
            [pixel1, pixel2]
        })
        .flatten();

    for i in dest.iter_mut().take(rgb_buf_size) {
        *i = match iter.next() {
            Some(v) => v,
            None => {
                return Err(anyhow!("Ran out of RGB YUYV values! (this should not happen, please file an issue: l1npengtul/nokhwa)"))
            }
        }
    }

    Ok(())
}

// equation from https://en.wikipedia.org/wiki/YUV#Converting_between_Y%E2%80%B2UV_and_RGB
/// Convert `YCbCr` 4:4:4 to a RGB888. [For further reading](https://en.wikipedia.org/wiki/YUV#Converting_between_Y%E2%80%B2UV_and_RGB)
#[allow(clippy::many_single_char_names)]
#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::cast_sign_loss)]
#[must_use]
#[inline]
pub fn yuyv444_to_rgb(y: i32, u: i32, v: i32) -> [u8; 3] {
    let c298 = (y - 16) * 298;
    let d = u - 128;
    let e = v - 128;
    let r = (c298 + 409 * e + 128) >> 8;
    let g = (c298 - 100 * d - 208 * e + 128) >> 8;
    let b = (c298 + 516 * d + 128) >> 8;
    [clamp_255(r), clamp_255(g), clamp_255(b)]
}

#[inline]
pub fn clamp_255(i: i32) -> u8{
    if i>255{
        255
    }else if i<0{
        0
    }else{
        i as u8
    }
}