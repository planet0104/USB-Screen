use crate::{
    monitor::{self, system_uptime, webcam_frame},
    nmc::ICONS,
    utils::{degrees_to_radians, execute_user_command, resize_image, test_resize_image},
};
use anyhow::Result;
use image::{
    buffer::ConvertBuffer, imageops::{resize, FilterType}, Rgba, RgbaImage
};
use log::error;
use offscreen_canvas::{measure_text, OffscreenCanvas, ResizeOption, RotateOption, WHITE};
use serde::{Deserialize, Serialize};
use std::{any::Any, sync::{atomic::{AtomicPtr, Ordering}, Arc, Mutex}};
use uuid::Uuid;

static DEFAULT_IMAGE: &[u8] = include_bytes!("../images/icon_photo.png");

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn from(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
        }
    }

    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    /** 扩大 */
    pub fn inflate(&mut self, dx: i32, dy: i32) {
        self.left -= dx;
        self.right += dx;
        self.top -= dy;
        self.bottom += dy;
    }

    pub fn deflate(&mut self, dx: i32, dy: i32) {
        self.left += dx;
        self.right -= dx;
        self.top += dy;
        self.bottom -= dy;
    }

    // 平移矩形
    pub fn offset(&mut self, dx: i32, dy: i32) {
        self.left += dx;
        self.right += dx;
        self.top += dy;
        self.bottom += dy;
    }

    pub fn contain(&self, x: i32, y: i32) -> bool {
        x >= self.left && x <= self.right && y >= self.top && y <= self.bottom
    }

    pub fn center(&self) -> (i32, i32) {
        (self.left + self.width() / 2, self.top + self.height() / 2)
    }

    // 设置矩形中心点
    pub fn set_center(&mut self, center_x: i32, center_y: i32) {
        let width = (self.right - self.left) / 2;
        let height = (self.bottom - self.top) / 2;
        self.left = center_x - width;
        self.right = center_x + width;
        self.top = center_y - height;
        self.bottom = center_y + height;
    }

    // 设置矩形左上角位置
    pub fn set_position(&mut self, left: i32, top: i32) {
        let width = self.right - self.left;
        let height = self.bottom - self.top;
        self.left = left;
        self.right = left + width;
        self.top = top;
        self.bottom = top + height;
    }

    // 设置矩形的尺寸（宽高）
    pub fn set_size(&mut self, width: i32, height: i32) {
        let center_x = (self.left + self.right) / 2;
        let center_y = (self.top + self.bottom) / 2;
        self.left = center_x - width / 2;
        self.right = center_x + width / 2;
        self.top = center_y - height / 2;
        self.bottom = center_y + height / 2;
    }

    // 设置矩形的尺寸（宽高）
    pub fn set_width_and_height(&mut self, width: i32, height: i32) {
        self.right = self.left + width;
        self.bottom = self.top + height;
    }
}

pub trait Widget {
    fn draw(&mut self, context: &mut OffscreenCanvas);
    fn id(&self) -> &str;
    fn index(&self) -> usize;
    fn set_index(&mut self, idx: usize);
    fn num_widget(&self) -> usize;
    fn set_num_widget(&mut self, num: usize);
    fn position(&self) -> &Rect;
    fn position_mut(&mut self) -> &mut Rect;
    fn type_name(&self) -> &str;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn is_text(&self) -> bool{
        self.type_name() != "images" && self.type_name() != "webcam"
    }
    fn is_image(&self) -> bool{
        self.type_name() == "images"
    }
    fn is_webcam(&self) -> bool{
        self.type_name() == "webcam"
    }
    fn get_label(&self) -> &str{
        if self.is_image() {
            "图像"
        }else if self.is_webcam() {
            "摄像头"
        } else {
            "文本"
        }
    }
}

#[derive(Default, Clone)]
pub struct CustomScriptStatus{
    pub loading: bool,
    pub result: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct TextWidget {
    pub id: String,
    pub text: String,
    pub prefix: String,
    pub color: [u8; 4],
    pub font_size: f32,
    pub position: Rect,
    pub type_name: String,
    // 在本类组件中，排序第几
    pub num_widget_index: usize,
    // 一共有多少个当前类型的组件
    pub num_widget: usize,
    pub tag1: String,
    pub tag2: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
    // 对齐方式 居中, 居左, 居右
    pub alignment: Option<String>,
    //自定义内容脚本(执行脚本后，获取到数据)
    pub custom_script: Option<String>,
    //这是执行命令完成后获得的数据
    #[serde(skip_serializing, skip_deserializing)]
    pub custom_script_data: Arc<Mutex<CustomScriptStatus>>
}

impl TextWidget {
    #[allow(unused)]
    pub fn new(x: i32, y: i32, type_name: &str, type_label: &str) -> Self {
        Self::new_with_text(x, y, type_name, type_label, "文本")
    }

    pub fn new_with_text(x: i32, y: i32, type_name: &str, type_label: &str, text: &str) -> Self {
        
        Self {
            id: Uuid::new_v4().to_string(),
            text: text.to_string(),
            prefix: if type_label.len() > 0 {
                format!("{type_label}:")
            } else {
                String::new()
            },
            color: WHITE.0,
            font_size: 14.,
            position: Rect::new(x, y, x + 1, y + 1),
            type_name: type_name.to_string(),
            num_widget_index: 0,
            num_widget: 1,
            tag1: "".to_string(),
            tag2: "".to_string(),
            alignment: None,
            width: None,
            height: None,
            custom_script: None,
            custom_script_data: Arc::new(Mutex::new(CustomScriptStatus{ loading: false, result: String::new()}))
        }
    }

    pub fn execute_user_command(&self, command:String){
        // 启动子线程，每秒更新 JSON 数据
        let data_clone = self.custom_script_data.clone();
        std::thread::spawn(move || {
            {
                //锁定
                let mut data = match data_clone.lock(){
                    Err(err) => {
                        error!("custom_script_data lock error:{err:?}");
                        return;
                    }
                    Ok(v) => v
                };
                data.loading = true;
            }
            // let t = Instant::now();
            let result = format!("{}", execute_user_command(&command).unwrap_or(String::from("脚本运行失败"))).replace("\r\n", "").replace("\n", "").replace("\r", "");
            // info!("脚本执行时间:{}ms {result}", t.elapsed().as_millis());
            {
                //锁定
                let mut data = match data_clone.lock(){
                    Err(err) => {
                        error!("custom_script_data lock error:{err:?}");
                        return;
                    }
                    Ok(v) => v
                };
                data.loading = false;
                data.result = result;
            }
        });
    }
}

impl Widget for TextWidget {
    fn draw(&mut self, context: &mut OffscreenCanvas) {
        
        let mut custom_script = None;
        if let Some(script) = self.custom_script.as_ref(){
            if script.trim().len() > 0{
                custom_script = Some(script);
            }
        }
        // 从自定义脚本中获取text
        if let Some(command) = custom_script{
            if let Ok(custom_script_data) = self.custom_script_data.try_lock(){
                if !custom_script_data.loading{
                    self.execute_user_command(command.clone());
                }
                self.text = custom_script_data.result.clone();
            }
        }else{
            if self.type_name != "text" {
                if let Some(text) = match self.type_name.as_str() {
                    "cpu" => monitor::cpu_brand(),
                    "memory" => monitor::memory_info(),
                    "memory_total" => monitor::memory_total(),
                    "memory_percent" => monitor::memory_percent(),
                    "swap" => monitor::swap_info(),
                    "swap_percent" => monitor::swap_percent(),
                    "system" => monitor::system_name(),
                    "version" => monitor::os_version(),
                    "kernel" => monitor::kernel_version(),
                    "host" => monitor::host_name(),
                    "cpu_freq" => monitor::cpu_clock_speed(None),
                    "cpu_usage" => {
                        if self.num_widget == 1 {
                            monitor::cpu_usage()
                        } else {
                            monitor::cpu_usage_percpu(self.num_widget_index)
                        }
                    }
                    "cpu_temp." => {
                        Some(monitor::cpu_temperature().unwrap_or(monitor::EMPTY_STRING.to_string()))
                    }
                    "cpu_cores_power" => {
                        Some(monitor::cpu_cores_power().unwrap_or(monitor::EMPTY_STRING.to_string()))
                    }
                    "cpu_package_power" => {
                        Some(monitor::cpu_package_power().unwrap_or(monitor::EMPTY_STRING.to_string()))
                    }
                    "cpu_fan" => Some(monitor::cpu_fan().unwrap_or(monitor::EMPTY_STRING.to_string())),
                    "gpu_fan" => Some(
                        monitor::gpu_fan(self.num_widget_index)
                            .unwrap_or(monitor::EMPTY_STRING.to_string()),
                    ),
                    "gpu_clock" => Some(
                        monitor::gpu_clocks(self.num_widget_index)
                            .unwrap_or(monitor::EMPTY_STRING.to_string()),
                    ),
                    "gpu_load" => Some(
                        monitor::gpu_load(self.num_widget_index)
                            .unwrap_or(monitor::EMPTY_STRING.to_string()),
                    ),
                    "gpu_memory_load" => Some(
                        monitor::gpu_memory_load(self.num_widget_index)
                            .unwrap_or(monitor::EMPTY_STRING.to_string()),
                    ),
                    "gpu_memory_total_mb" => Some(
                        monitor::gpu_memory_total_mb(self.num_widget_index)
                            .unwrap_or(monitor::EMPTY_STRING.to_string()),
                    ),
                    "gpu_memory_total_gb" => Some(
                        monitor::gpu_memory_total_gb(self.num_widget_index)
                            .unwrap_or(monitor::EMPTY_STRING.to_string()),
                    ),
                    "gpu_temp." => Some(
                        monitor::gpu_temperature(self.num_widget_index)
                            .unwrap_or(monitor::EMPTY_STRING.to_string()),
                    ),
                    "gpu_cores_power" => {
                        Some(monitor::gpu_cores_power().unwrap_or(monitor::EMPTY_STRING.to_string()))
                    }
                    "gpu_package_power" => {
                        Some(monitor::gpu_package_power().unwrap_or(monitor::EMPTY_STRING.to_string()))
                    }
                    "num_cpu" => monitor::num_cpus(),
                    "num_process" => monitor::num_process(),
                    "disk_usage" => monitor::disk_usage(self.num_widget_index),
                    "date" => Some(monitor::date()),
                    "local_ip" => monitor::local_ip_addresses(),
                    "net_ip" => monitor::net_ip_address(),
                    "net_ip_info" => monitor::net_ip_info(),
                    "time" => Some(monitor::time()),
                    "weekday" => Some(monitor::chinese_weekday()),
                    "lunar_year" => Some(monitor::lunar_year()),
                    "lunar_date" => Some(monitor::lunar_date()),
                    "weather" => match monitor::weather_info() {
                        None => Some(monitor::EMPTY_STRING.to_string()),
                        Some(w) => {
                            match self.tag1.as_str() {
                                "1" => Some(format!("{}", w.station.city)),         //城市
                                "2" => Some(format!("{}℃", w.weather.temperature)), //气温
                                "3" => Some(format!("{}℃", w.wind.direct)),         //风向
                                "4" => Some(format!("{}", w.wind.power)),           //风力
                                "5" => Some(format!("{}级", w.wind.speed)),         //风级
                                "6" => Some(format!("{}", w.weather.img)),          //图标
                                _ => Some(format!("{}", w.weather.info)),
                            }
                        }
                    },
                    "uptime" => {
                        let uptime = system_uptime();
                        let uptime_str = match self.tag1.as_str() {
                            //运行分钟数
                            "1" => Some(format!("{}", uptime.minutes)),
                            //运行小时数
                            "2" => Some(format!("{}", uptime.hours)),
                            //运行天数
                            "3" => Some(format!("{}", uptime.days)),
                            //运行秒数
                            _ => Some(format!("{}", uptime.seconds)),
                        };
                        uptime_str
                    },
                    "disk_read_speed" => monitor::disk_speed_per_sec().map(|(r, _w)| r),
                    "disk_write_speed" => monitor::disk_speed_per_sec().map(|(_r, w)| w),
                    "received_speed" => monitor::network_speed_per_sec().map(|(r, _t)| r),
                    "transmitted_speed" => monitor::network_speed_per_sec().map(|(_r, t)| t),
                    _ => None,
                } {
                    if self.text != text && text != monitor::EMPTY_STRING {
                        self.text = text;
                    }
                }
            }    
        }

        //天气渲染成图标
        if self.type_name == "weather" && self.tag1 == "6" {
            let img_idx = self.text.parse::<usize>().unwrap_or(0);
            let o = ResizeOption {
                nwidth: self.font_size as u32,
                nheight: self.font_size as u32,
                filter: FilterType::Triangle,
            };
            let (mut x, mut y) = self.position.center();
            x -= self.font_size as i32 / 2;
            y -= self.font_size as i32 / 2;
            context.draw_image_at(&ICONS[img_idx], x, y, Some(o), None);
        } else if self.type_name != "weather" && self.type_name != "uptime" && (self.tag1 == "1" || self.tag1 == "2") {
            //是否渲染成进度条
            let percent = self
                .text
                .replace("%", "")
                .replace("°C", "")
                .parse::<f32>()
                .unwrap_or(0.);

            let width = self.width.unwrap_or(self.font_size as i32 * 5);
            let height = self.height.unwrap_or(self.font_size as i32);

            //水平进度条
            if self.tag1 == "1" {
                let mut rect_width = (width as f32 * (percent / 100.)) as i32;
                if rect_width <= 0 {
                    rect_width = 1;
                }
                if self.font_size <= 2. {
                    self.font_size = 2.;
                }
                let rect = offscreen_canvas::Rect::from(
                    self.position.left,
                    self.position.top,
                    rect_width,
                    height,
                );
                context.fill_rect(rect, Rgba(self.color));
            }else{
                //垂直进度条
                let mut rect_height = (height as f32 * (percent / 100.)) as i32;
                if rect_height <= 0 {
                    rect_height = 1;
                }
                if self.font_size <= 2. {
                    self.font_size = 2.;
                }
                let rect = offscreen_canvas::Rect::from(
                    self.position.left,
                    self.position.top+(height-rect_height),
                    width,
                    rect_height,
                );
                context.fill_rect(rect, Rgba(self.color));
            }
        } else {
            if self.font_size <= 4. {
                self.font_size = 4.;
            }
            let text = format!("{}{}", self.prefix, self.text);
            let text_rect = context.measure_text(&text, self.font_size);
            let width = self.width.unwrap_or(text_rect.width());
            let height = self.height.unwrap_or(text_rect.height());
            let alignment = self.alignment.clone().unwrap_or("".to_string());
            if self.width.is_some() && alignment.len() > 0{
                self.position.set_width_and_height(width, height);
                let text_rect = measure_text(&text, self.font_size, context.font());
                if alignment == "居中"{
                    context.draw_text(
                        &text,
                        Rgba(self.color),
                        self.font_size,
                        self.position.center().0 - text_rect.width()/2,
                        self.position.top,
                    );
                }else if alignment == "居左"{
                    context.draw_text(
                        &text,
                        Rgba(self.color),
                        self.font_size,
                        self.position.left,
                        self.position.top,
                    );
                }else if alignment == "居右"{
                    context.draw_text(
                        &text,
                        Rgba(self.color),
                        self.font_size,
                        self.position.right - text_rect.width(),
                        self.position.top,
                    );
                }
            }else{
                //居中方式调整文本位置
                self.position.set_size(width, height);
                context.draw_text(
                    &text,
                    Rgba(self.color),
                    self.font_size,
                    self.position.left,
                    self.position.top,
                );
            }
        }
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn position_mut(&mut self) -> &mut Rect {
        &mut self.position
    }

    fn type_name(&self) -> &str {
        &self.type_name
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn position(&self) -> &Rect {
        &self.position
    }

    fn index(&self) -> usize {
        self.num_widget_index
    }

    fn set_index(&mut self, idx: usize) {
        self.num_widget_index = idx;
    }

    fn num_widget(&self) -> usize {
        self.num_widget
    }

    fn set_num_widget(&mut self, num: usize) {
        self.num_widget = num;
    }
}

#[derive(Default, Clone, Deserialize, Serialize)]
pub struct ImageData {
    pub width: u32,
    pub height: u32,
    pub frames: Vec<Vec<u8>>,
}

impl ImageData {
    pub fn load(data: &[u8], max_size: (u32, u32)) -> Result<Self> {
        let format = image::guess_format(data)?;
        Ok(match format {
            image::ImageFormat::Gif => {
                let mut frames = vec![];

                let mut gif_opts = gif::DecodeOptions::new();
                // Important:
                gif_opts.set_color_output(gif::ColorOutput::Indexed);
                let mut decoder = gif_opts.read_info(data)?;

                //计算最大图像大小
                let (width, height) = test_resize_image(
                    decoder.width() as u32,
                    decoder.height() as u32,
                    max_size.0,
                    max_size.1,
                );
                let scale = width as f32 / decoder.width() as f32;

                let mut screen = gif_dispose::Screen::new_decoder(&decoder);

                while let Some(frame) = decoder.read_next_frame()? {
                    screen.blit_frame(&frame)?;
                    let rgba = screen.pixels_rgba();
                    let mut pixels = Vec::with_capacity(rgba.width() * rgba.height() * 4);
                    for pixel in rgba.pixels() {
                        pixels.extend_from_slice(&[pixel.r, pixel.g, pixel.b, pixel.a]);
                    }
                    let img =
                        RgbaImage::from_raw(rgba.width() as u32, rgba.height() as u32, pixels)
                            .unwrap();
                    //等比例缩放
                    let nw = img.width() as f32 * scale;
                    let nh = img.height() as f32 * scale;
                    let img: RgbaImage = img;
                    let img =
                        image::imageops::resize(&img, nw as u32, nh as u32, FilterType::Triangle);
                    frames.push(img.into_raw());
                }

                Self {
                    width,
                    height,
                    frames,
                }
            }
            _ => {
                let image = image::load_from_memory(data).unwrap().to_rgba8();
                let resized = resize_image(
                    &image,
                    max_size.0,
                    max_size.1,
                    image::imageops::FilterType::Triangle,
                );
                Self {
                    width: resized.width(),
                    height: resized.height(),
                    frames: vec![resized.to_vec()],
                }
            }
        })
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct ImageWidget {
    pub id: String,
    pub image_data: ImageData,
    pub rotation: f32,
    pub position: Rect,
    pub type_name: String,
    pub frame_index: usize,
    //是否为纯色
    pub color: Option<[u8; 4]>,
    pub num_widget_index: usize,
    // 一共有多少个当前类型的组件
    pub num_widget: usize,
    pub tag1: Option<String>,
    pub tag2: Option<String>,
}

impl ImageWidget {
    pub fn from_v10(img:v10::ImageWidget) -> Self{
        Self { id: img.id, image_data: img.image_data, rotation: img.rotation, position: img.position, type_name: img.type_name, frame_index: img.frame_index, color: img.color,
            num_widget_index: img.num_widget_index, num_widget: img.num_widget, tag1: None, tag2: None }
    }
    
    pub fn new(x: i32, y: i32, type_name: &str) -> Self {
        let image = image::load_from_memory(DEFAULT_IMAGE).unwrap().to_rgba8();
        let image = resize(&image, 50, 50, FilterType::Nearest);
        let (w, h) = (image.width(), image.height());
        Self {
            id: Uuid::new_v4().to_string(),
            image_data: ImageData {
                width: w,
                height: h,
                frames: vec![image.to_vec()],
            },
            rotation: 0.,
            position: Rect::from(x - w as i32 / 2, y - h as i32 / 2, w as i32, h as i32),
            type_name: type_name.to_string(),
            color: None,
            frame_index: 0,
            num_widget_index: 0,
            num_widget: 1,
            tag1: None,
            tag2: None,
        }
    }
}

impl Widget for ImageWidget {
    fn draw(&mut self, context: &mut OffscreenCanvas) {
        if let Some(color) = self.color.as_ref() {
            let rect = offscreen_canvas::Rect::from(
                self.position.left,
                self.position.top,
                self.position.width(),
                self.position.height(),
            );
            context.fill_rect(rect, Rgba(*color));
        }
        //是否是相机
        else if self.type_name == "webcam"{
            //获取相机图像
            if let Some(image) = webcam_frame(){
                let src =
                    offscreen_canvas::Rect::new(0, 0, image.width() as i32, image.height() as i32);

                //按照宽度比例绘制
                let width = self.position.width();
                let height = ((image.height() as f32 / image.width() as f32)*width as f32) as i32;
                
                let pos = offscreen_canvas::Rect::from(
                    self.position.left,
                    self.position.top,
                    width,
                    height,
                );

                context.draw_image_with_src_and_dst(&image.convert(), &src, &pos, FilterType::Nearest);
            }else{
                //未打开相机，显示白色
                let rect = offscreen_canvas::Rect::from(
                    self.position.left,
                    self.position.top,
                    self.position.width(),
                    self.position.height(),
                );
                context.fill_rect(rect, WHITE);
            }
        }else {
            if self.frame_index >= self.image_data.frames.len(){
                self.frame_index = self.image_data.frames.len()-1;
            }
            let image = RgbaImage::from_raw(
                self.image_data.width,
                self.image_data.height,
                self.image_data.frames[self.frame_index].clone(),
            ).unwrap_or(RgbaImage::new(30, 30));
            let src =
                offscreen_canvas::Rect::new(0, 0, image.width() as i32, image.height() as i32);
            let pos = offscreen_canvas::Rect::from(
                self.position.left,
                self.position.top,
                self.position.width(),
                self.position.height(),
            );

            if self.rotation == 0.{
                //不旋转
                context.draw_image_with_src_and_dst(&image, &src, &pos, FilterType::Nearest);
            }else{
                let option = RotateOption::from(
                    (
                        self.position.width() as f32 / 2.,
                        self.position.height() as f32 / 2.,
                    ),
                    degrees_to_radians(self.rotation),
                );
                context.draw_image_with_src_and_dst_and_rotation(&image, &src, &pos, option);
            }
            self.frame_index += 1;
            if self.frame_index >= self.image_data.frames.len() {
                self.frame_index = 0;
            }
        }
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn position_mut(&mut self) -> &mut Rect {
        &mut self.position
    }

    fn type_name(&self) -> &str {
        &self.type_name
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn position(&self) -> &Rect {
        &self.position
    }

    fn index(&self) -> usize {
        self.num_widget_index
    }

    fn set_index(&mut self, idx: usize) {
        self.num_widget_index = idx;
    }

    fn num_widget(&self) -> usize {
        self.num_widget
    }

    fn set_num_widget(&mut self, num: usize) {
        self.num_widget = num;
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub enum SaveableWidget {
    TextWidget(TextWidget),
    ImageWidget(ImageWidget),
}

//老版本
pub mod v10{
    use super::*;

    #[derive(Clone, Deserialize, Serialize)]
    pub enum SaveableWidget {
        TextWidget(super::TextWidget),
        ImageWidget(ImageWidget),
    }

    #[derive(Clone, Deserialize, Serialize)]
    pub struct ImageWidget {
        pub id: String,
        pub image_data: ImageData,
        pub rotation: f32,
        pub position: Rect,
        pub type_name: String,
        pub frame_index: usize,
        //是否为纯色
        pub color: Option<[u8; 4]>,
        pub num_widget_index: usize,
        // 一共有多少个当前类型的组件
        pub num_widget: usize,
    }

    impl Widget for ImageWidget {
        fn draw(&mut self, context: &mut OffscreenCanvas) {
            if let Some(color) = self.color.as_ref() {
                let rect = offscreen_canvas::Rect::from(
                    self.position.left,
                    self.position.top,
                    self.position.width(),
                    self.position.height(),
                );
                context.fill_rect(rect, Rgba(*color));
            }else {
                if self.frame_index >= self.image_data.frames.len(){
                    self.frame_index = self.image_data.frames.len()-1;
                }
                let image = RgbaImage::from_raw(
                    self.image_data.width,
                    self.image_data.height,
                    self.image_data.frames[self.frame_index].clone(),
                ).unwrap_or(RgbaImage::new(30, 30));
                let src =
                    offscreen_canvas::Rect::new(0, 0, image.width() as i32, image.height() as i32);
                let pos = offscreen_canvas::Rect::from(
                    self.position.left,
                    self.position.top,
                    self.position.width(),
                    self.position.height(),
                );

                if self.rotation == 0.{
                    //不旋转
                    context.draw_image_with_src_and_dst(&image, &src, &pos, FilterType::Nearest);
                }else{
                    let option = RotateOption::from(
                        (
                            self.position.width() as f32 / 2.,
                            self.position.height() as f32 / 2.,
                        ),
                        degrees_to_radians(self.rotation),
                    );
                    context.draw_image_with_src_and_dst_and_rotation(&image, &src, &pos, option);
                }
                self.frame_index += 1;
                if self.frame_index >= self.image_data.frames.len() {
                    self.frame_index = 0;
                }
            }
        }

        fn id(&self) -> &str {
            &self.id
        }

        fn position_mut(&mut self) -> &mut Rect {
            &mut self.position
        }

        fn type_name(&self) -> &str {
            &self.type_name
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }

        fn position(&self) -> &Rect {
            &self.position
        }

        fn index(&self) -> usize {
            self.num_widget_index
        }

        fn set_index(&mut self, idx: usize) {
            self.num_widget_index = idx;
        }

        fn num_widget(&self) -> usize {
            self.num_widget
        }

        fn set_num_widget(&mut self, num: usize) {
            self.num_widget = num;
        }
    }
}