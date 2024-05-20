use std::path::PathBuf;

use image::{imageops::FilterType, RgbaImage};

pub fn degrees_to_radians(degrees: f32) -> f32 {
    degrees * std::f32::consts::PI / 180.0
}

pub fn resize_image(
    image: &RgbaImage,
    max_width: u32,
    max_height: u32,
    filter: FilterType,
) -> RgbaImage {
    let (width, height) = image.dimensions();

    // 计算缩放比例
    let scale_factor = if width > height {
        max_width as f32 / width as f32
    } else {
        max_height as f32 / height as f32
    };

    // 计算新的尺寸，确保不会超过最大值
    let new_width = (width as f32 * scale_factor).round() as u32;
    let new_height: u32 = (height as f32 * scale_factor).round() as u32;

    // 使用resize方法进行缩放
    let img = image::imageops::resize(image, new_width, new_height, filter);
    img
}

pub fn test_resize_image(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    // 计算缩放比例
    let scale_factor = if width > height {
        max_width as f32 / width as f32
    } else {
        max_height as f32 / height as f32
    };

    // 计算新的尺寸，确保不会超过最大值
    let new_width = (width as f32 * scale_factor).round() as u32;
    let new_height: u32 = (height as f32 * scale_factor).round() as u32;

    (new_width, new_height)
}

//解析字体名称
pub fn get_font_name(ttf: PathBuf, max_char: usize) -> anyhow::Result<String> {
    // 初始化系统字体源
    let font_data = std::fs::read(ttf)?;

    let face = ttf_parser::Face::parse(&font_data, 0)?;

    let mut family_names = Vec::new();
    for name in face.names() {
        if name.name_id == ttf_parser::name_id::FULL_NAME && name.is_unicode() {
            if let Some(family_name) = name.to_string() {
                let language = name.language();
                family_names.push(format!(
                    "{} ({}, {})",
                    family_name,
                    language.primary_language(),
                    language.region()
                ));
            }
        }
    }

    let family_name = if family_names.len() > 1 && family_names[1].contains("Chinese") {
        family_names[1].to_string()
    } else {
        family_names.get(0).unwrap_or(&String::new()).to_string()
    };

    let mut new_name = String::new();
    for c in family_name.chars() {
        if new_name.chars().count() < max_char {
            new_name.push(c);
        } else {
            break;
        }
    }
    Ok(new_name)
}
