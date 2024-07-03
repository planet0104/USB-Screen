use std::{collections::HashMap, path::PathBuf};

use crate::{
    monitor::{self, WebcamInfo},
    nmc::CITIES,
    widgets::{ImageWidget, SaveableWidget, TextWidget, Widget},
};
use anyhow::{anyhow, Result};
use bincode::{Decode, Encode};
use log::info;
use lz4_flex::{compress_prepend_size, decompress_size_prepended};
use offscreen_canvas::{Font, FontSettings, OffscreenCanvas, BLACK};
use serde::{Deserialize, Serialize};

pub static DEFAULT_FONT: &[u8] = include_bytes!("../fonts/VonwaonBitmap-16px.ttf");

#[derive(Clone, Debug)]
pub struct ScreenSize {
    pub name: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Encode, Decode, Deserialize, Serialize)]
pub struct SaveableScreen {
    pub width: u32,
    pub height: u32,
    pub model: String,
    //最大刷新帧率
    pub fps: f32,
    //指定链接设备编号
    pub device_address: Option<String>,
    pub widgets: Vec<SaveableWidget>,
    pub font: Option<Vec<u8>>,
    pub font_name: String
}

#[derive(Clone, Encode, Decode, Deserialize, Serialize)]
pub struct SaveableScreenV10 {
    pub width: u32,
    pub height: u32,
    pub model: String,
    pub widgets: Vec<super::widgets::v10::SaveableWidget>,
    pub font: Option<Vec<u8>>,
    pub font_name: String,
}

pub struct ScreenRender {
    pub width: u32,
    pub height: u32,
    pub model: String,
    pub widgets: Vec<Box<dyn Widget>>,
    pub canvas: OffscreenCanvas,
    pub font_name: String,
    pub font: Option<Vec<u8>>,
    pub fps: f32,
    pub device_address: Option<String>
}

impl ScreenRender {
    pub fn new(
        model: String,
        width: u32,
        height: u32,
        font_file: Option<&[u8]>,
        font_name: String,
    ) -> Result<Self> {
        let font_file_clone = font_file.clone();
        let font_file = font_file.unwrap_or(DEFAULT_FONT);
        let font =
            Font::from_bytes(font_file, FontSettings::default()).map_err(|err| anyhow!("{err}"))?;
        Ok(Self {
            canvas: OffscreenCanvas::new(width, height, font),
            width,
            height,
            model,
            font_name,
            font: font_file_clone.map(|v| v.to_vec()),
            widgets: vec![],
            fps: 10.,
            device_address: None,
        })
    }

    pub fn set_font(&mut self, font_file: Option<&[u8]>, font_name: String) -> Result<()> {
        let font_file_clone = font_file.clone();
        let font_file = font_file.unwrap_or(DEFAULT_FONT);
        let font =
            Font::from_bytes(font_file, FontSettings::default()).map_err(|err| anyhow!("{err}"))?;
        self.canvas = OffscreenCanvas::new(self.width, self.height, font);
        self.font = font_file_clone.map(|v| v.to_vec());
        self.font_name = font_name;
        Ok(())
    }

    pub fn setup_monitor(&mut self) -> Result<()> {
        //在点击的地方添加一个对象
        for widget in &mut self.widgets {
            info!("setup_monitor:{}", widget.type_name());
            match widget.type_name() {
                "memory" | "memory_total" | "memory_percent" | "swap" | "swap_percent" => {
                    monitor::watch_memory(true)?
                }
                "webcam" =>{
                    if let Some(widget) = widget.as_any_mut().downcast_mut::<ImageWidget>() {
                        info!("webcam: tag1={:?}", widget.tag1);
                        monitor::watch_webcam(Some(WebcamInfo{
                            width: self.width,
                            height: self.height,
                            index: widget.tag1.as_ref().unwrap_or(&String::new()).parse().unwrap_or(0),
                            fps: self.fps as u32
                        }))?
                    }
                }
                "cpu" | "cpu_usage" => monitor::watch_cpu(true)?,
                "cpu_freq" => monitor::watch_cpu_clock_speed(true)?,
                "cpu_temp." => monitor::watch_cpu_temperatures(true)?,
                "cpu_cores_power" | "gpu_cores_power" => monitor::watch_cpu_power(true)?,
                "cpu_package_power" | "gpu_package_power" => monitor::watch_cpu_power(true)?,
                "cpu_fan" => monitor::watch_cpu_fan(true)?,
                "gpu_fan" => monitor::watch_gpu_fan(true)?,
                "gpu_clock" => monitor::watch_gpu_clock_speed(true)?,
                "gpu_load" | "gpu_memory_load" | "gpu_memory_total_mb" | "gpu_memory_total_gb" => monitor::watch_gpu_load(true)?,
                "gpu_temp." => monitor::watch_gpu_temperatures(true)?,
                "num_process" => monitor::watch_process(true)?,
                "disk_usage" => monitor::watch_disk(true)?,
                "net_ip" | "net_ip_info" => monitor::watch_net_ip(true)?,
                "disk_read_speed" => monitor::watch_disk_speed(true)?,
                "disk_write_speed" => monitor::watch_disk_speed(true)?,
                "received_speed" => monitor::watch_network_speed(true)?,
                "transmitted_speed" => monitor::watch_network_speed(true)?,
                "weather" => {
                    if let Some(widget) = widget.as_any_mut().downcast_mut::<TextWidget>() {
                        if widget.tag2.len() > 0 {
                            //查询对应的城市
                            info!("更新天气，查询对应的城市: tag2={}", widget.tag2);
                            if let Some(city) = CITIES.iter().find(|c| c.city == widget.tag2) {
                                monitor::watch_weather(Some(city.clone()))?
                            }
                        }
                    }
                }
                _ => (),
            }
        }
        Ok(())
    }

    pub fn render(&mut self) {
        //更新索引
        let mut map = HashMap::new();
        for w in self.widgets.iter_mut() {
            if !map.contains_key(w.type_name()) {
                map.insert(w.type_name().to_string(), 0);
            } else {
                *map.get_mut(w.type_name()).unwrap() += 1;
            }
            w.set_index(*map.get_mut(w.type_name()).unwrap());
        }
        for w in self.widgets.iter_mut() {
            w.set_num_widget(*map.get_mut(w.type_name()).unwrap());
        }
        self.canvas.clear(BLACK);
        for widget in &mut self.widgets {
            widget.draw(&mut self.canvas);
        }
    }

    pub fn add_widget(
        &mut self,
        type_name: &str,
        type_label: &str,
        x: i32,
        y: i32,
    ) -> Option<String> {
        if type_name.len() == 0 {
            return None;
        }

        let widget: Box<dyn Widget> = if type_name == "images" || type_name == "webcam" {
            Box::new(ImageWidget::new(x, y, &type_name))
        } else {
            let mut text_index = 1;
            for w in self.widgets.iter_mut() {
                if let Some(_) = w.as_any_mut().downcast_mut::<TextWidget>() {
                    text_index += 1;
                }
            }
            Box::new(TextWidget::new_with_text(
                x,
                y,
                &type_name,
                &type_label,
                &format!("文本{text_index}"),
            ))
        };
        let id = widget.id().to_string();
        self.widgets.push(widget);
        Some(id)
    }

    pub fn find_widget(&mut self, uuid: &str) -> Option<(usize, &mut Box<dyn Widget>)> {
        self.widgets
            .iter_mut()
            .enumerate()
            .find(|(_idx, w)| w.id() == uuid)
    }

    #[allow(unused)]
    pub fn find_widget_by_index(&mut self, index: usize) -> Option<(usize, &mut Box<dyn Widget>)> {
        self.widgets
            .iter_mut()
            .enumerate()
            .find(|(idx, w)| *idx == index)
    }

    pub fn width(&self) -> u32 {
        self.canvas.width()
    }

    pub fn height(&self) -> u32 {
        self.canvas.height()
    }

    //尝试使用bindcode解析老版本screen文件
    pub fn load_from_file(&mut self, file: PathBuf) -> Result<()> {
        let compressed = std::fs::read(file)?;
        let uncompressed = decompress_size_prepended(&compressed)?;
        let saveable: Result<(SaveableScreenV10, usize), bincode::error::DecodeError> =
            bincode::decode_from_slice(&uncompressed, bincode::config::standard());

        //解析失败尝试使用V2解析
        if saveable.is_err(){
            self.load_from_file_v2(&uncompressed)?;
            return Ok(());
        }
        
        let (saveable, _) = saveable?;
        self.width = saveable.width;
        self.height = saveable.height;
        self.canvas =
            OffscreenCanvas::new(saveable.width, saveable.height, self.canvas.font().clone());
        if let Some(font) = saveable.font {
            self.set_font(Some(&font), saveable.font_name)?;
        }
        self.widgets.clear();
        for w in saveable.widgets {
            match w {
                crate::widgets::v10::SaveableWidget::TextWidget(txt) => {
                    self.widgets.push(Box::new(txt));
                }
                crate::widgets::v10::SaveableWidget::ImageWidget(img) => {
                    self.widgets.push(Box::new(ImageWidget::from_v10(img)));
                }
            }
        }
        Ok(())
    }

    //使用json解析screen文件
    pub fn load_from_file_v2(&mut self, uncompressed: &[u8]) -> Result<()> {
        let saveable:SaveableScreen = serde_json::from_str(&String::from_utf8(uncompressed.to_vec())?)?;
        // let saveable: Result<(SaveableScreen, usize), bincode::error::DecodeError> =
        //     bincode::decode_from_slice(&uncompressed, bincode::config::standard());
        // let (saveable, _) = saveable?;
        self.width = saveable.width;
        self.height = saveable.height;
        self.fps = saveable.fps;
        self.device_address = saveable.device_address;
        self.canvas =
            OffscreenCanvas::new(saveable.width, saveable.height, self.canvas.font().clone());
        if let Some(font) = saveable.font {
            self.set_font(Some(&font), saveable.font_name)?;
        }
        self.widgets.clear();
        for w in saveable.widgets {
            match w {
                SaveableWidget::TextWidget(txt) => {
                    self.widgets.push(Box::new(txt));
                }
                SaveableWidget::ImageWidget(img) => {
                    self.widgets.push(Box::new(img));
                }
            }
        }
        Ok(())
    }

    pub fn new_from_file(file: &[u8]) -> Result<ScreenRender> {
        let uncompressed = decompress_size_prepended(&file)?;
        let saveable: Result<(SaveableScreenV10, usize), bincode::error::DecodeError> =
            bincode::decode_from_slice(&uncompressed, bincode::config::standard());
        if saveable.is_err(){
            return Self::new_from_file_v2(&uncompressed);
        }
        let (saveable, _) = saveable?;
        let model = saveable.model;
        let mut render =
            ScreenRender::new(model, saveable.width, saveable.height, None, String::new())?;
        if let Some(font) = saveable.font {
            render.set_font(Some(&font), saveable.font_name)?;
        }
        render.widgets.clear();
        for w in saveable.widgets {
            match w {
                crate::widgets::v10::SaveableWidget::TextWidget(txt) => {
                    render.widgets.push(Box::new(txt));
                }
                crate::widgets::v10::SaveableWidget::ImageWidget(img) => {
                    render.widgets.push(Box::new(ImageWidget::from_v10(img)));
                }
            }
        }
        Ok(render)
    }

    pub fn new_from_file_v2(uncompressed: &[u8]) -> Result<ScreenRender> {
        let saveable:SaveableScreen = serde_json::from_str(&String::from_utf8(uncompressed.to_vec())?)?;

        let model = saveable.model;
        let mut render =
            ScreenRender::new(model, saveable.width, saveable.height, None, String::new())?;
        if let Some(font) = saveable.font {
            render.set_font(Some(&font), saveable.font_name)?;
        }
        render.fps = saveable.fps;
        render.device_address = saveable.device_address;
        render.widgets.clear();
        for w in saveable.widgets {
            match w {
                SaveableWidget::TextWidget(txt) => {
                    render.widgets.push(Box::new(txt));
                }
                SaveableWidget::ImageWidget(img) => {
                    render.widgets.push(Box::new(img));
                }
            }
        }
        Ok(render)
    }

    //改为json格式存储，这样添加了新的字段不影响解析原有格式的screen文件
    pub fn to_json(&mut self) -> Result<Vec<u8>> {
        let mut font = self.font.clone();
        let font_name = self.font_name.clone();
        if font_name == "凤凰点阵"{
            font = None;
        }
        let mut saveable = SaveableScreen {
            width: self.width,
            height: self.height,
            model: self.model.clone(),
            font,
            font_name,
            widgets: vec![],
            fps: self.fps,
            device_address: self.device_address.clone()
        };
        for idx in 0..self.widgets.len() {
            if let Some(widget) = self.widgets[idx].as_any_mut().downcast_mut::<TextWidget>() {
                saveable
                    .widgets
                    .push(SaveableWidget::TextWidget(widget.clone()));
            }
            if let Some(widget) = self.widgets[idx].as_any_mut().downcast_mut::<ImageWidget>() {
                saveable
                    .widgets
                    .push(SaveableWidget::ImageWidget(widget.clone()));
            }
        }
        let json = serde_json::to_string(&saveable)?;
        let contents = json.as_bytes();
        info!("压缩前:{}k", contents.len() / 1024);
        //压缩
        let compressed = compress_prepend_size(contents);
        info!("压缩后:{}k", compressed.len() / 1024);
        Ok(compressed)
    }
}
