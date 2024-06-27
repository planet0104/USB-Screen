use anyhow::Result;
use hex_color::HexColor;
use image::buffer::ConvertBuffer;
use image::{imageops::resize, RgbImage};
use log::{error, info};
use offscreen_canvas::{OffscreenCanvas, BLUE, WHITE};
use rfd::{FileDialog, MessageDialog};
use slint::private_unstable_api::re_exports::KeyEvent;
use slint::{
    Brush, Color, Image, Model, SharedPixelBuffer, SharedString, Timer, TimerMode, VecModel, Weak,
};
use std::time::Instant;
use once_cell::sync::Lazy;
use std::{
    cell::RefCell,
    fs::File,
    io::{Read, Write},
    rc::Rc,
    str::FromStr,
    sync::{Arc, Mutex},
};

use crate::usb_screen::{self, UsbScreen, UsbScreenInfo};
use crate::{
    nmc::CITIES,
    screen::{ScreenRender, ScreenSize, DEFAULT_FONT},
    utils::get_font_name,
    widgets::{ImageData, ImageWidget, TextWidget, Widget},
};

struct CurrentUsbScreen{
    info: UsbScreenInfo,
    screen: UsbScreen
}

// 当前打开的屏幕
static SCREEN: Lazy<Mutex<Option<CurrentUsbScreen>>> = Lazy::new(|| {
    Mutex::new(None)
});


slint::include_modules!();

struct CanvasEditorContext {
    app: Weak<CanvasEditor>,
    screen: ScreenRender,
    temp_image: Arc<Mutex<Option<ImageData>>>,
    screens: Vec<ScreenSize>,
    list_model: Rc<VecModel<WidgetObject>>,
    active_id: Option<String>,
    is_drag: bool,
    start_drag_dx: i32,
    start_drag_dy: i32,
    picker_img: RgbImage,
    fps: i32,
    last_frame_time: Option<Instant>,
    devices: Vec<UsbScreenInfo>
}

impl CanvasEditorContext {
    fn new(app: Weak<CanvasEditor>) -> Self {
        let list_model = Rc::new(VecModel::from(vec![]));
        let win = app.unwrap();
        win.set_object_list(list_model.clone().into());

        win.set_active_widget_type_name("".into());

        let picker_img = image::load_from_memory(include_bytes!("../images/picker.png"))
            .unwrap()
            .to_rgb8();

        let screens = vec![ScreenSize {
            name: "ST7735".into(),
            width: 160,
            height: 128,
        },
        ScreenSize {
            name: "ST7789".into(),
            width: 320,
            height: 240,
        }];

        let screen_names = Rc::new(VecModel::from(
            screens
                .iter()
                .map(|screen| format!("{ } {}x{}", screen.name, screen.width, screen.height).into())
                .collect::<Vec<SharedString>>(),
        ));

        win.set_screen_names(screen_names.into());
        win.set_screen_name(format!(
            "{ } {}x{}",
            screens[0].name, screens[0].width, screens[0].height
        )
        .into());
        win.set_screen_width(screens[0].width as f32);
        win.set_screen_height(screens[0].height as f32);

        CanvasEditorContext {
            app,
            screen: ScreenRender::new(
                screens[0].name.clone(),
                screens[0].width,
                screens[0].height,
                Some(DEFAULT_FONT),
                "凤凰点阵".to_string(),
            )
            .unwrap(),
            temp_image: Arc::new(Mutex::new(None)),
            active_id: None,
            is_drag: false,
            start_drag_dx: 0,
            start_drag_dy: 0,
            list_model,
            screens,
            picker_img,
            fps: 10,
            last_frame_time: None,
            devices: vec![],
        }
    }

    pub fn active_widget(&mut self) -> Option<&mut Box<dyn Widget>> {
        let active_id = self.active_id.clone();
        match &active_id {
            Some(uuid) => Some(self.screen.find_widget(uuid)?.1),
            None => None,
        }
    }

    pub fn update_device_list(&mut self){
        let connected_device: &[&UsbScreenInfo] = if let Ok(screen) = SCREEN.lock(){
            if let Some(device) = screen.as_ref(){
                &[&device.info.clone()]
            }else{
                &[]
            }
        }else{
            &[]
        };
        self.devices = usb_screen::find_all_device(connected_device);
        let device_list = Rc::new(VecModel::from(
            self.devices
                .iter()
                .map(|dev| format!("{} {}x{}", dev.label, dev.width, dev.height).into())
                .collect::<Vec<SharedString>>(),
        ));
        if self.devices.len() == 0{
            device_list.push("未找到".into());
        }
        let app = self.app.unwrap();
        app.set_device_list(device_list.into());
        let mut dev_index:i32 = -1;
        let current_name = app.get_device_name().to_string();
        for (idx, dev) in self.devices.iter().enumerate(){
            if dev.label == current_name{
                dev_index = idx as i32;
                break;
            }
        }

        if dev_index == -1 && self.devices.len()>0{
            let dev = &self.devices[0];
            dev_index = 0;
            app.set_device_name(format!("{} {}x{}", dev.label, dev.width, dev.height).into());
        }

        if self.devices.len() == 0{
            app.set_device_name("未找到".into());
        }

        //连接当前设备
        if dev_index >= 0{
            let dev = self.devices[dev_index as usize].clone();
            std::thread::spawn(move ||{
                if let Ok(mut screen) = SCREEN.lock(){
                    if screen.is_some() && screen.as_ref().unwrap().info.label == dev.label{
                        return;
                    }
    
                    match UsbScreen::open(dev.clone()){
                        Ok(s) => {
                            screen.replace(CurrentUsbScreen { info: dev.clone(), screen: s });
                        }
                        Err(err) => {
                            error!("屏幕打开失败:{:?}", err);
                        }
                    }
                }
            });
        }
    }

    pub fn render_screen(&mut self) {
        self.screen.render();
        //绘制选中的框
        if let Some(active_id) = self.active_id.as_ref() {
            for widget in &mut self.screen.widgets {
                if widget.id() == active_id {
                    let rect = widget.position();
                    let mut rect = offscreen_canvas::Rect {
                        left: rect.left,
                        top: rect.top,
                        right: rect.right,
                        bottom: rect.bottom,
                    };

                    //进度条按照tag2为宽度
                    if let Some(widget) = widget.as_any_mut().downcast_mut::<TextWidget>() {
                        if widget.type_name != "weather" && widget.type_name != "uptime" && widget.tag1 == "1" {
                            let width = widget
                                .tag2
                                .parse::<i32>()
                                .unwrap_or(widget.font_size as i32 * 5);
                            rect = offscreen_canvas::Rect::from(
                                rect.left,
                                rect.top,
                                width,
                                rect.height(),
                            );
                        }
                    }

                    if rect.width() <= 1 {
                        rect.set_size(2, rect.height())
                    }
                    if rect.height() <= 1 {
                        rect.set_size(rect.width(), 2)
                    }

                    //其他按照位置为大小
                    self.screen.canvas.stroke_rect(rect, BLUE);
                    self.screen.canvas.stroke_rect(
                        offscreen_canvas::Rect::new(
                            rect.left - 1,
                            rect.top - 1,
                            rect.right + 1,
                            rect.bottom + 1,
                        ),
                        WHITE,
                    );
                    break;
                }
            }
        }
        let image_data = self.screen.canvas.image_data();
        let buf = SharedPixelBuffer::clone_from_slice(
            &image_data,
            self.screen.width(),
            self.screen.height(),
        );
        self.app
            .unwrap()
            .set_canvas_frame(slint::Image::from_rgba8(buf));

        if let Some(last_frame_time) = self.last_frame_time.as_ref(){
            if (last_frame_time.elapsed().as_millis() as i32) < 1000/self.fps{
                return;
            }
        }

        //发送到USB屏幕
        let frame: RgbImage = self.screen.canvas.image_data().convert();
        std::thread::spawn(move ||{
            if let Ok(mut screen) = SCREEN.lock(){
                if let Some(device) = screen.as_mut(){
                    let _ = device.screen.draw_rgb_image(0,0,&frame);
                }
            }
        });
        //更新最后时间
        self.last_frame_time = Some(Instant::now());
    }

    fn on_mouse_click(&mut self, mouse_x: f32, mouse_y: f32, image_width: f32, image_height: f32) {
        // info!("on_mouse_click 鼠标位置:{mouse_x}x{mouse_y}");
        let app = self.app.unwrap();

        if self.is_drag {
            self.is_drag = false;
            info!("结束拖拽.");
            return;
        }
        let index = app.get_widget_type_index();
        let (x, y) = Self::get_real_pos(&self.screen, mouse_x, mouse_y, image_width, image_height);

        if index == 0 {
            self.set_active_widget(x, y);
        } else {
            self.add_widget(x, y);
        }
    }

    fn on_mouse_move(
        &mut self,
        mouse_x: f32,
        mouse_y: f32,
        image_width: f32,
        image_height: f32,
        pressed: bool,
    ) {
        let app = self.app.unwrap();
        let (x, y) = Self::get_real_pos(&self.screen, mouse_x, mouse_y, image_width, image_height);

        if pressed {
            if self.is_drag {
                let (x, y) = (x + self.start_drag_dx, y + self.start_drag_dy);
                let active_widget = match self.active_widget() {
                    None => return,
                    Some(v) => v,
                };

                active_widget.position_mut().set_center(x, y);
                app.set_active_widget_x(format!("{x}").into());
                app.set_active_widget_y(format!("{y}").into());
            } else {
                self.is_drag = true;
                let active_widget = match self.active_widget() {
                    None => return,
                    Some(v) => v,
                };
                let start_drag_dx = active_widget.position().center().0 - x;
                let start_drag_dy = active_widget.position().center().1 - y;
                self.start_drag_dx = start_drag_dx;
                self.start_drag_dy = start_drag_dy;
            }
        }
    }

    fn on_update_widget_position(&mut self) {
        let app = self.app.unwrap();
        // info!("更新位置:{}x{}", x_str.as_str(), y_str.as_str());

        let widget = match self.active_widget() {
            None => return,
            Some(v) => v,
        };

        let width_str = app.get_active_widget_width();
        let height_str = app.get_active_widget_height();
        let rotate_str = app.get_active_widget_rotation();

        let x_str = app.get_active_widget_x().to_string();
        let y_str = app.get_active_widget_y().to_string();
        let x: i32 = x_str.parse().unwrap_or(widget.position().center().0);
        let y: i32 = y_str.parse().unwrap_or(widget.position().center().1);
        let (mut nw, mut nh) = (
            width_str
                .parse::<i32>()
                .unwrap_or(widget.position().width()),
            height_str
                .parse::<i32>()
                .unwrap_or(widget.position().height()),
        );
        if nw <= 0 || nw > 500 {
            nw = widget.position().width();
        }
        if nh < 0 || nh > 500 {
            nh = widget.position().height();
        }
        if nw <= 2 {
            nw = 2;
        }
        if nh <= 2 {
            nh = 2;
        }

        app.set_active_widget_width(format!("{nw}").into());
        app.set_active_widget_height(format!("{nh}").into());
        widget.position_mut().set_center(x, y);

        if let Some(widget) = widget.as_any_mut().downcast_mut::<ImageWidget>() {
            widget.position_mut().set_size(nw, nh);
            widget.rotation = rotate_str.parse().unwrap_or(widget.rotation);
            app.set_active_widget_rotation(format!("{}", widget.rotation as i32).into());
        }
    }

    fn on_update_widget_text(&mut self) {
        let app = self.app.unwrap();
        let prefix = app.get_active_widget_prefix();
        let text = app.get_active_widget_text();
        let font_size = app.get_active_widget_font_size();
        let color = app.get_active_widget_color();
        if text.len() == 0 {
            return;
        }

        let widget = match self
            .active_widget()
            .and_then(|w| w.as_any_mut().downcast_mut::<TextWidget>())
        {
            None => return,
            Some(v) => v,
        };

        let old_font_size = widget.font_size;

        widget.font_size = font_size.parse().unwrap_or(old_font_size);
        if widget.font_size < 5. {
            widget.font_size = 5.;
        }
        widget.text = text.to_string();
        widget.prefix = prefix.to_string();
        if let Ok(color) = HexColor::from_str(&color.to_string()) {
            widget.color[0] = color.r;
            widget.color[1] = color.g;
            widget.color[2] = color.b;
            widget.color[3] = color.a;
        }

        let widget_id = widget.id().to_string();
        let text = widget.text.to_string();
        let prefix = widget.prefix.to_string();

        self.update_widget_edit_text();

        let (idx, mut model) = match self.find_widget_model(&widget_id).clone() {
            Some(v) => v,
            None => return,
        };
        model.text = text.into();
        model.prefix = prefix.into();

        // info!("更新了文本:{:?}", model);

        self.list_model.set_row_data(idx, model);
    }

    pub fn find_widget_model(&mut self, uuid: &str) -> Option<(usize, WidgetObject)> {
        self.list_model
            .iter()
            .enumerate()
            .find(|(_, w)| w.uuid == uuid)
    }

    fn on_update_widget_image_color(&mut self) {
        let color_str = self.app.unwrap().get_active_widget_color().to_string();
        let mut color = None;
        if color_str.len() > 0 {
            if let Ok(c) = HexColor::from_str(&color_str) {
                color = Some([c.r, c.g, c.b, c.a]);
            }
        }

        if let Some(widget) = self
            .active_widget()
            .and_then(|w| w.as_any_mut().downcast_mut::<ImageWidget>())
        {
            widget.color = color;
        }
    }

    fn on_update_widget_text_color(&mut self) {
        let color_str = self.app.unwrap().get_active_widget_color_str().to_string();
        let mut color = None;
        if color_str.len() > 0 {
            if let Ok(c) = HexColor::from_str(&color_str) {
                color = Some([c.r, c.g, c.b, c.a]);
            }
        }

        if let (Some(color), Some(widget)) = (color, self
            .active_widget()
            .and_then(|w| w.as_any_mut().downcast_mut::<TextWidget>()))
        {
            widget.color = color;
        }
    }

    fn on_update_widget_image(&mut self) {
        let temp_image_clone = self.temp_image.clone();
        let (screen_width, screen_height) = (self.screen.width, self.screen.height);
        let app_clone = self.app.clone();
        std::thread::spawn(move || {
            let file = match FileDialog::new()
                .add_filter("图像", &["png", "bmp", "jpg", "jpeg", "gif"])
                .pick_file()
            {
                None => return,
                Some(path) => path,
            };
            let mut file_data = vec![];
            let result = File::open(file).map(|mut f| f.read_to_end(&mut file_data));
            if let (Ok(Ok(img)), Ok(mut tmp)) = (
                result.map(|_| ImageData::load(&file_data, (screen_width, screen_height))),
                temp_image_clone.lock(),
            ) {
                info!(
                    "选择了图片，最终大小:{}x{} 帧数:{} 帧大小:{}",
                    img.width,
                    img.height,
                    img.frames.len(),
                    img.frames[0].len()
                );
                tmp.replace(img);
                let _ = app_clone.upgrade_in_event_loop(move |app| {
                    app.invoke_new_image_ready();
                });
            }
        });
    }

    fn on_update_widget_tags(&mut self) {
        let app = self.app.unwrap();
        let tag1 = app.get_active_widget_tag1();
        let tag2 = app.get_active_widget_tag2();
        
        if let Some(widget) = self.active_widget()
            .and_then(|w| w.as_any_mut().downcast_mut::<ImageWidget>())
        {
            if widget.is_webcam(){
                //更新摄像头
                widget.tag1 = Some(tag1.to_string());
            }
        }

        if let Some(widget) = self
            .active_widget()
            .and_then(|w| w.as_any_mut().downcast_mut::<TextWidget>())
        {
            widget.tag1 = tag1.to_string();

            //更新天气
            if widget.type_name == "weather" {
                let city_name = tag2.to_string();
                //查询城市名称
                if city_name.trim().len() > 0 {
                    let mut find_city = None;
                    for city in CITIES.iter() {
                        if city.city.contains(city_name.as_str()) {
                            find_city = Some(city.clone());
                            break;
                        }
                    }
    
                    if let Some(city) = find_city {
                        widget.tag2 = city.city.clone();
                        self.app
                            .unwrap()
                            .set_active_widget_tag2(city.city.clone().into());
                        let _ = self.screen.setup_monitor();
                        //更新所有日期组件的tag2
                        for w in self.screen.widgets.iter_mut() {
                            if let Some(widget) = w.as_any_mut().downcast_mut::<TextWidget>() {
                                widget.tag2 = city.city.clone();
                            }
                        }
                        //刷新ui
                        self.refresh_model_text();
                        self.app.unwrap().set_active_widget_tag2(city.city.into());
                    }
                }
    
                self.app.unwrap().set_active_widget_tag1(tag1);
            } else {
                //更新文字、进度条类型
                widget.tag2 = tag2.to_string();
                self.app.unwrap().set_active_widget_tag2(tag2);
            }
        }
    }

    fn on_new_image_ready(&mut self) {
        let image = self.temp_image.lock();
        if image.is_err() {
            return;
        }
        let image = image.unwrap().take();
        if image.is_none() {
            return;
        }
        let tmp_img = image.unwrap();
        let (w, h) = (tmp_img.width, tmp_img.height);

        let (image, width, height) = match self
            .active_widget()
            .and_then(|w| w.as_any_mut().downcast_mut::<ImageWidget>())
        {
            None => return,
            Some(widget) => {
                widget.position_mut().set_size(w as i32, h as i32);
                widget.image_data = tmp_img;
                (
                    Image::from_rgba8(SharedPixelBuffer::clone_from_slice(
                        &widget.image_data.frames[0],
                        widget.image_data.width,
                        widget.image_data.height,
                    )),
                    widget.position.width(),
                    widget.position.height(),
                )
            }
        };

        let app = self.app.unwrap();
        app.set_active_widget_image(image);
        app.set_active_widget_width(format!("{width}").into());
        app.set_active_widget_height(format!("{height}").into());
    }

    fn on_select_widget(&mut self, uuid: SharedString) {
        self.active_id = Some(uuid.to_string());
        self.show_active_widget();
    }

    fn show_active_widget(&mut self) {
        let app = self.app.unwrap();

        if let Some(widget) = self
            .active_widget()
            .and_then(|w| w.as_any_mut().downcast_mut::<ImageWidget>())
        {
            // info!("当前选中了图像:{}", widget.id);
            app.set_active_widget_type_name(widget.type_name.as_str().into());
            app.set_active_widget_rotation(format!("{}", widget.rotation as i32).into());
            app.set_active_widget_width(format!("{}", widget.position().width()).into());
            app.set_active_widget_height(format!("{}", widget.position().height()).into());
            app.set_active_widget_image(Image::from_rgba8(SharedPixelBuffer::clone_from_slice(
                &widget.image_data.frames[0],
                widget.image_data.width,
                widget.image_data.height,
            )));
            app.set_active_widget_uuid(SharedString::from(widget.id()));
            app.set_active_widget_x(format!("{}", widget.position().center().0).into());
            app.set_active_widget_y(format!("{}", widget.position().center().1).into());
            return;
        }

        if let Some(widget) = self
            .active_widget()
            .and_then(|w| w.as_any_mut().downcast_mut::<TextWidget>())
        {
            app.set_active_widget_type_name(widget.type_name.as_str().into());
            // info!("当前选中了文本:{}", widget.id);
            app.set_active_widget_uuid(SharedString::from(widget.id()));
            app.set_active_widget_x(format!("{}", widget.position().left).into());
            app.set_active_widget_y(format!("{}", widget.position().top).into());
        }
        self.update_widget_edit_text();

        if let Some(id) = self.active_id.as_ref() {
            let widget_num = self.get_widget_num() as i32;
            let mut select_index = widget_num - self.get_widget_index(id).unwrap_or(0) as i32 - 1;
            if select_index < 0 {
                select_index = 0;
            }
            app.invoke_update_list_view_scroll(widget_num, select_index);
        }
    }

    //选中一个对象
    fn set_active_widget(&mut self, x: i32, y: i32) {
        if let Some(old_active_id) = self.active_id.clone() {
            //如果有选中的，那么选中这个uuid的下一个组件
            let mut clicked_uuid: Vec<String> = self
                .screen
                .widgets
                .iter()
                .filter_map(|v| {
                    if v.position().contain(x, y) {
                        Some(v.id().to_string())
                    } else {
                        None
                    }
                })
                .collect();

            if clicked_uuid.len() == 0 {
                self.active_id = None;
                let app = self.app.unwrap();
                app.set_active_widget_type_name(SharedString::from(""));
                app.set_active_widget_uuid(SharedString::from(""));
                return;
            }

            if clicked_uuid.len() == 1 {
                self.active_id = Some(clicked_uuid.remove(0));
                self.show_active_widget();
                return;
            }

            let mut old_idx = 0;
            for (i, cid) in clicked_uuid.iter().enumerate() {
                if *cid == old_active_id {
                    old_idx = i;
                    break;
                }
            }

            let new_uuid = if old_idx < clicked_uuid.len() - 1 {
                clicked_uuid.remove(old_idx + 1)
            } else {
                clicked_uuid.remove(0)
            };

            self.active_id = Some(new_uuid);
        } else {
            //如果没有选中的，那么按顺序选择第一个
            for w in &self.screen.widgets {
                if w.position().contain(x, y) {
                    self.active_id = Some(w.id().to_string());
                    break;
                }
            }
        }

        self.show_active_widget();
    }

    fn update_widget_edit_text(&mut self) {
        let app = self.app.unwrap();
        let widget = match self
            .active_widget()
            .and_then(|w| w.as_any_mut().downcast_mut::<TextWidget>())
        {
            None => return,
            Some(v) => v,
        };
        app.set_active_widget_text(SharedString::from(&widget.text));
        app.set_active_widget_tag1(SharedString::from(&widget.tag1));
        app.set_active_widget_tag2(SharedString::from(&widget.tag2));
        app.set_active_widget_font_size(format!("{}", widget.font_size as i32).into());
        app.set_active_widget_prefix(SharedString::from(&widget.prefix));
        app.set_active_widget_color(Color::from_argb_u8(
            widget.color[3],
            widget.color[0],
            widget.color[1],
            widget.color[2],
        ));
        app.set_active_widget_color_str(SharedString::from(
            HexColor::rgba(
                widget.color[0],
                widget.color[1],
                widget.color[2],
                widget.color[3],
            )
            .display_rgba()
            .to_string(),
        ));
    }

    //在点击的地方添加一个组件
    fn add_widget(&mut self, x: i32, y: i32) {
        let app = self.app.unwrap();
        let widget_type_name = app.get_widget_type_name();
        let widget_type_label = if widget_type_name.as_str() == "weather" {
            SharedString::new()
        } else {
            app.get_widget_type_label()
        };
        // info!("add_widget name:{} 位置:{x}x{y}", widget_type_name.as_str());

        self.active_id = self
            .screen
            .add_widget(&widget_type_name, &widget_type_label, x, y);

        if self.active_id.is_none() {
            return;
        }
        let uuid = self.active_id.clone().unwrap();
        let mut text = "".to_string();
        let mut prefix = "".to_string();
        if let Some((idx, w)) = self.screen.find_widget(&uuid) {
            if w.is_text() {
                if let Some(widget) = w.as_any_mut().downcast_mut::<TextWidget>() {
                    text = widget.text.to_string();
                    prefix = widget.prefix.to_string();
                }
            }

            let model = WidgetObject {
                index: idx as i32,
                name: SharedString::from(w.get_label()),
                type_name: SharedString::from(w.type_name()),
                uuid: SharedString::from(w.id()),
                text: SharedString::from(&text),
                prefix: SharedString::from(&prefix),
                tag1: SharedString::from(""),
                tag2: SharedString::from(""),
            };
            info!("添加了一个:{:?}", model);

            self.list_model.push(model);

            app.set_widget_type_index(0);
            let ret = self.screen.setup_monitor();
            info!("更新监视器:{:?}", ret);

            self.show_active_widget();
        }
    }

    fn get_widget_index(&self, uuid: &str) -> Option<usize> {
        self.screen
            .widgets
            .iter()
            .position(|item| item.id() == uuid)
    }

    fn get_widget_num(&self) -> usize {
        self.screen.widgets.len()
    }

    // 每隔一秒钟更新一次widget文本
    fn refresh_model_text(&mut self) {
        let models = self.list_model.clone();
        for (idx, w) in self.screen.widgets.iter_mut().rev().enumerate() {
            let mut text = "".to_string();
            let mut prefix = "".to_string();
            if w.is_text() {
                if let Some(widget) = w.as_any_mut().downcast_mut::<TextWidget>() {
                    text = widget.text.to_string();
                    prefix = widget.prefix.to_string();
                }
            }

            let uuid = w.id().to_string();
            let (tag1, tag2) = {
                let found = models.iter().enumerate().find(|(_, w)| w.uuid == uuid);
                found
                    .map(|(_i, m)| (m.tag1.clone(), m.tag2.clone()))
                    .unwrap_or((SharedString::default(), SharedString::default()))
            };

            let model = WidgetObject {
                index: idx as i32,
                name: SharedString::from(w.get_label()),
                type_name: SharedString::from(w.type_name()),
                uuid: SharedString::from(w.id()),
                text: SharedString::from(&text),
                prefix: SharedString::from(&prefix),
                tag1,
                tag2,
            };

            self.list_model.set_row_data(idx, model);
        }
    }

    // 往数组后移动对象，使对象位于它的渲染上层
    fn move_back_widget(&mut self, uuid: SharedString) {
        let uuid = uuid.as_str();

        //当前选中的索引
        let widget_index = match self
            .screen
            .widgets
            .iter()
            .position(|item| item.id() == uuid)
        {
            None => return,
            Some(i) => i,
        };
        //如果已经位于最上层，不处理
        if widget_index >= self.screen.widgets.len() - 1 {
            return;
        }
        //下一个索引
        self.screen.widgets.swap(widget_index, widget_index + 1);
        self.refresh_model_text();
    }

    // 往数组前移动对象，使对对象位于它的渲染下层
    fn move_up_widget(&mut self, uuid: SharedString) {
        let uuid = uuid.as_str();

        //当前选中的索引
        let widget_index = match self
            .screen
            .widgets
            .iter()
            .position(|item| item.id() == uuid)
        {
            None => return,
            Some(i) => i,
        };
        //如果已经位于最下层，不处理
        if widget_index == 0 || self.screen.widgets.len() == 1 {
            return;
        }
        //下一个索引
        self.screen.widgets.swap(widget_index - 1, widget_index);
        self.refresh_model_text();
    }

    fn delete_widget(&mut self, uuid: &str) {
        let widget_index = match self
            .screen
            .widgets
            .iter()
            .position(|item| item.id() == uuid)
        {
            None => return,
            Some(i) => i,
        };

        self.screen.widgets.remove(widget_index);
        self.list_model.remove(widget_index);
        self.refresh_model_text();
        if let Some(active_uuid) = self.active_id.as_ref() {
            if active_uuid == uuid {
                self.active_id = None;
                self.app
                    .unwrap()
                    .set_active_widget_uuid(SharedString::from(""));
            }
        }
    }

    fn clone_widget(&mut self, uuid: &str) {
        let widget_index = match self
            .screen
            .widgets
            .iter()
            .position(|item| item.id() == uuid)
        {
            None => return,
            Some(i) => i,
        };

        let app = self.app.unwrap();

        let widget_type_name:SharedString = self.screen.widgets[widget_index].type_name().into();
        let widget_type_label = if widget_type_name.as_str() == "weather" {
            SharedString::new()
        } else {
            app.get_widget_type_label()
        };

        self.active_id = self
            .screen
            .add_widget(&widget_type_name, &widget_type_label, self.screen.widgets[widget_index].position().left, self.screen.widgets[widget_index].position().top);

        if self.active_id.is_none() {
            return;
        }
        let uuid = self.active_id.clone().unwrap();
        let mut text = "".to_string();
        let mut prefix = "".to_string();
        let mut tag1 = "".to_string();
        let mut tag2 = "".to_string();

        let mut text_widget_clone = None;
        let mut image_widget_clone = None;

        if let Some(ref_text_widget) = self.screen.widgets[widget_index].as_any_mut().downcast_mut::<TextWidget>() {
            text_widget_clone = Some(ref_text_widget.clone());
        }
        if let Some(ref_image_widget) = self.screen.widgets[widget_index].as_any_mut().downcast_mut::<ImageWidget>() {
            image_widget_clone = Some(ref_image_widget.clone());
        }

        if let Some((idx, w)) = self.screen.find_widget(&uuid) {

            if let Some(text_widget) = w.as_any_mut().downcast_mut::<TextWidget>() {
                *text_widget = text_widget_clone.unwrap();
                text = text_widget.text.to_string();
                prefix = text_widget.prefix.to_string();
                tag1 = text_widget.tag1.to_string();
                tag2 = text_widget.tag2.to_string();
                text_widget.id = uuid.clone();
            }
            if let Some(image_widget) = w.as_any_mut().downcast_mut::<ImageWidget>() {
                *image_widget = image_widget_clone.unwrap();
                image_widget.id = uuid.clone();
            }

            w.position_mut().offset(5, 5);

            let model = WidgetObject {
                index: idx as i32,
                name: SharedString::from(w.get_label()),
                type_name: SharedString::from(w.type_name()),
                uuid: SharedString::from(w.id()),
                text: SharedString::from(&text),
                prefix: SharedString::from(&prefix),
                tag1: SharedString::from(&tag1),
                tag2: SharedString::from(&tag2),
            };
            info!("添加了一个:{:?}", model);

            self.list_model.push(model);

            app.set_widget_type_index(0);
            let ret = self.screen.setup_monitor();
            info!("更新监视器:{:?}", ret);

            self.show_active_widget();
        }
    }

    // 鼠标缩放
    fn on_screen_mouse_scroll(&mut self, _dx: f32, dy: f32) {
        let app = self.app.unwrap();

        //往下滑动dy>0否则dy<0
        if let Some(widget) = self
            .active_widget()
            .and_then(|w| w.as_any_mut().downcast_mut::<TextWidget>())
        {
            if dy > 0. {
                widget.font_size += 0.5;
            } else {
                widget.font_size -= 0.5;
            }
            app.set_active_widget_font_size(format!("{}", widget.font_size as i32).into());
        }
        if let Some(widget) = self
            .active_widget()
            .and_then(|w| w.as_any_mut().downcast_mut::<ImageWidget>())
        {
            if widget.position.width() <= 1 || widget.position.height() <= 1 {
                return;
            }
            if dy > 0. {
                widget.position.inflate(2, 2);
            } else {
                if widget.position.width() > 4 && widget.position.height() > 4 {
                    widget.position.deflate(2, 2);
                }
            }
            app.set_active_widget_width(format!("{}", widget.position.width()).into());
            app.set_active_widget_height(format!("{}", widget.position.height()).into());
        }
    }

    // 键盘事件
    fn on_screen_key_event(&mut self, event: KeyEvent) {
        let app = self.app.unwrap();

        let mut delete_uuid = String::new();
        if let Some(widget) = self.active_widget() {
            let char = event.text.chars().next().unwrap_or(' ');
            if char == '\u{7f}' {
                delete_uuid = widget.id().to_string();
            }
            if char == 'w' || char == '\u{f700}' {
                widget.position_mut().offset(0, -1);
            }
            if char == 's' || char == '\u{f701}' {
                widget.position_mut().offset(0, 1);
            }
            if char == 'a' || char == '\u{f702}' {
                widget.position_mut().offset(-1, 0);
            }
            if char == 'd' || char == '\u{f703}' {
                widget.position_mut().offset(1, 0);
            }
            let (x, y) = widget.position().center();
            app.set_active_widget_x(format!("{x}").into());
            app.set_active_widget_y(format!("{y}").into());
        }

        if delete_uuid.len() > 0 {
            self.delete_widget(&delete_uuid);
        }
    }

    fn on_change_screen(&mut self, index: i32) {
        let screen = &self.screens[index as usize];
        
        let width_scale = screen.width as f32 / self.screen.width as f32;
        let height_scale = screen.height as f32 / self.screen.height as f32;

        info!("on_change_screen: {screen:?} width_scale={width_scale} height_scale={height_scale}");

        self.screen.width = screen.width;
        self.screen.height = screen.height;
        
        //修改画布大小
        self.screen.canvas = OffscreenCanvas::new(
            screen.width,
            screen.height,
            self.screen.canvas.font().clone(),
        );

        //修改元素大小
        for idx in 0..self.screen.widgets.len() {
            if self.screen.widgets[idx].is_text() {
                if let Some(widget) = self.screen.widgets[idx]
                    .as_any_mut()
                    .downcast_mut::<TextWidget>()
                {
                    //重新设置进度条设置宽度
                    if widget.type_name != "weather" && widget.type_name != "uptime" && widget.tag1 == "1"{
                        let tag2 = widget.tag2.clone();
                        let width = tag2.parse::<f32>().unwrap_or(widget.font_size * 5.);
                        widget.tag2 = format!("{}", (width_scale * width) as i32);
                        let new_left = widget.position().left as f32 * width_scale;
                        let new_top = widget.position().top as f32 * height_scale;
                        widget.position_mut().set_position(new_left as i32, new_top as i32);
                        widget.font_size = height_scale * widget.font_size as f32;
                    }else{
                        let pos = widget.position_mut();
                        let (x, y) = pos.center();
                        pos.set_center((x as f32 * width_scale) as i32, (y as f32 * height_scale) as i32);
                        widget.font_size = height_scale * widget.font_size as f32;
                    }
                }
            }
            if !self.screen.widgets[idx].is_text() {
                if let Some(widget) = self.screen.widgets[idx]
                    .as_any_mut()
                    .downcast_mut::<ImageWidget>()
                {
                    let pos = widget.position_mut();
                    let (x, y) = pos.center();
                    let new_width = pos.width() as f32 * width_scale;
                    let new_height = pos.height() as f32 * height_scale;
                    let dw = (new_width - pos.width() as f32) /2.;
                    let dh = (new_height - pos.height() as f32) /2.;
                    pos.inflate(dw as i32, dh as i32);
                    pos.set_center((x as f32 * width_scale) as i32, (y as f32 * height_scale) as i32);
                }
            }
        }

        let app = self.app.unwrap();
        app.set_screen_name(format!(
            "{ } {}x{}",
            screen.name, screen.width, screen.height
        )
        .into());
        app.set_screen_width(screen.width as f32);
        app.set_screen_height(screen.height as f32);
        //刷新监听器
        let _ = self.screen.setup_monitor();
    }

    fn on_save_screen(&mut self) {
        //检查是否有打开的屏幕，并且跟当前屏幕大小一致，保存至配置文件中
        let mut size_fit = false;
        if let Ok(current_device) = SCREEN.lock(){
            if let Some(screen) = current_device.as_ref(){
                if screen.info.width == self.screen.width as u16 && screen.info.height == self.screen.height as u16{
                    self.screen.device_address = Some(screen.info.address.clone());
                    size_fit = true;
                }
            }
        }
        self.screen.fps = self.fps;
        //错误的屏幕大小要清空
        if !size_fit{
            self.screen.device_address = None;
        }
        
        match self.screen.to_json() {
            Ok(file_data) => {
                let file_name = format!("{}x{}.screen", self.screen.width, self.screen.height);
                std::thread::spawn(move || {
                    let dlg = rfd::FileDialog::new()
                        .add_filter("screen", &["screen"])
                        .set_file_name(file_name);
                    if let Some(file) = dlg.save_file() {
                        if let Ok(mut f) = std::fs::File::create(file) {
                            let _ = f.write_all(&file_data);
                        }
                    }
                });
            }
            Err(err) => {
                error!("{:?}", err);
                MessageDialog::new()
                    .set_description(format!("{:?}", err))
                    .set_buttons(rfd::MessageButtons::Ok)
                    .show();
            }
        }
    }

    fn on_open_screen(&mut self) {
        let dlg = rfd::FileDialog::new().add_filter("screen", &["screen"]);
        if let Some(file) = dlg.pick_file() {
            match self.screen.load_from_file(file) {
                Ok(()) => {
                    //更新帧率
                    let fps_str = format!("{}", self.screen.fps);
                    self.on_change_fps(SharedString::from(&fps_str));
                    //更新显示的屏幕大小
                    let app = self.app.unwrap();
                    for s in &self.screens{
                        if s.width == self.screen.width && s.height == self.screen.height{
                            app.set_screen_name(format!(
                                "{ } {}x{}",
                                s.name, s.width, s.height
                            )
                            .into());
                            app.set_screen_width(s.width as f32);
                            app.set_screen_height(s.height as f32);
                            break;
                        }
                    }
                    //更新显示列表
                    self.list_model = Rc::new(VecModel::from(vec![]));
                    for idx in 0..self.screen.widgets.len() {
                        let mut text = "".to_string();
                        let mut prefix = "".to_string();
                        if self.screen.widgets[idx].is_text() {
                            if let Some(widget) = self.screen.widgets[idx]
                                .as_any_mut()
                                .downcast_mut::<TextWidget>()
                            {
                                text = widget.text.to_string();
                                prefix = widget.prefix.to_string();
                            }
                        }

                        let model = WidgetObject {
                            index: idx as i32,
                            name: SharedString::from(self.screen.widgets[idx].get_label()),
                            type_name: SharedString::from(self.screen.widgets[idx].type_name()),
                            uuid: SharedString::from(self.screen.widgets[idx].id()),
                            text: SharedString::from(&text),
                            prefix: SharedString::from(&prefix),
                            tag1: SharedString::from(""),
                            tag2: SharedString::from(""),
                        };
                        info!("添加了一个:{:?}", model);

                        self.list_model.push(model);
                    }
                    //刷新监听器
                    let _ = self.screen.setup_monitor();
                    //清空选中的widget
                    let app = self.app.unwrap();
                    app.set_font_name(self.screen.font_name.clone().into());
                    app.set_object_list(self.list_model.clone().into());
                    app.set_active_widget_type_name("".into());
                    app.set_active_widget_uuid("".into());
                }
                Err(err) => {
                    error!("{:?}", err);
                    MessageDialog::new()
                        .set_description(format!("{:?}", err))
                        .set_buttons(rfd::MessageButtons::Ok)
                        .show();
                }
            }
        }
    }

    fn on_open_font(&mut self) {
        let dlg = rfd::FileDialog::new().add_filter("字体文件", &["ttf"]);
        if let Some(file_path) = dlg.pick_file() {
            if let Ok(buf) = std::fs::read(file_path.clone()) {
                let font_name = get_font_name(file_path, 7).unwrap();
                if let Ok(_) = self.screen.set_font(Some(&buf), font_name.to_string()) {
                    self.app.unwrap().set_font_name(font_name.into());
                }
            }
        } else {
            let _ = self.screen.set_font(None, "凤凰点阵".to_string());
            self.app
                .unwrap()
                .set_font_name(self.screen.font_name.clone().into());
        }
    }

    //从图像中选择颜色
    fn on_color_picker_choose_color(&mut self, x: f32, y: f32) -> Brush {
        if self.picker_img.width() != 200 || self.picker_img.height() != 221 {
            self.picker_img = resize(
                &self.picker_img,
                300,
                221,
                image::imageops::FilterType::Triangle,
            );
        }
        let pixel = self.picker_img.get_pixel(x as u32, y as u32).clone();

        let type_name = self
            .active_widget()
            .and_then(|w| Some(w.type_name()))
            .unwrap_or("");

        if type_name == "images" {
            self.update_image_widget_color(Some([pixel[0], pixel[1], pixel[2], 255]));
        } else {
            //设置当前文本的字体颜色字符串
            self.update_text_widget_color(pixel[0], pixel[1], pixel[2]);
        }
        Brush::SolidColor(Color::from_rgb_u8(pixel[0], pixel[1], pixel[2]))
    }

    fn on_color_picker_brightness_change(&mut self) {
        // 限制亮度因子在0.0到1.0之间
        let app = self.app.unwrap();
        let color = app.get_picker_color();
        let brightness_factor = app.get_picker_brightness();
        let brightness_factor = (100. - brightness_factor) / 100.;
        info!("brightness_factor={brightness_factor}");

        // 将RGB值转换为浮点数并应用亮度调整
        let r = ((color.color().red() as f32 / 255.0) * brightness_factor * 255.0) as u8;
        let g = ((color.color().green() as f32 / 255.0) * brightness_factor * 255.0) as u8;
        let b = ((color.color().blue() as f32 / 255.0) * brightness_factor * 255.0) as u8;

        let type_name = self
            .active_widget()
            .and_then(|w| Some(w.type_name()))
            .unwrap_or("");

        if type_name == "images" {
            self.update_image_widget_color(Some([r, g, b, 255]));
        } else {
            //设置当前文本的字体颜色字符串
            self.update_text_widget_color(r, g, b);
        }
    }

    //设置当前文本的字体颜色字符串
    fn update_text_widget_color(&mut self, r: u8, g: u8, b: u8) {
        if let Some(widget) = self
            .active_widget()
            .and_then(|w| w.as_any_mut().downcast_mut::<TextWidget>())
        {
            widget.color = [r, g, b, 255];
            let app = self.app.unwrap();
            app.set_active_widget_color(Color::from_argb_u8(255, r, g, b));
            app.set_active_widget_color_str(SharedString::from(
                HexColor::rgba(r, g, b, 255).display_rgba().to_string(),
            ));
        }
    }

    fn update_image_widget_color(&mut self, color: Option<[u8; 4]>) {
        if let Some(widget) = self
            .active_widget()
            .and_then(|w| w.as_any_mut().downcast_mut::<ImageWidget>())
        {
            widget.color = color.clone();
        }
        let app = self.app.unwrap();
        if let Some(color) = color {
            app.set_active_widget_image_color_str(SharedString::from(
                HexColor::rgba(color[0], color[1], color[2], color[3])
                    .display_rgba()
                    .to_string(),
            ));
        } else {
            app.set_active_widget_image_color_str(SharedString::from(""));
        }
    }

    fn get_real_pos(
        screen: &ScreenRender,
        mouse_x: f32,
        mouse_y: f32,
        image_width: f32,
        image_height: f32,
    ) -> (i32, i32) {
        let scale_x = screen.width() as f32 / image_width;
        let scale_y = screen.height() as f32 / image_height;

        let x = (mouse_x * scale_x) as i32;
        let y = (mouse_y * scale_y) as i32;
        (x, y)
    }

    fn on_save_capture(&mut self) {
        let image = self.screen.canvas.image_data().clone();
        let file_name = format!("{}x{}.png", self.screen.width, self.screen.height);
        std::thread::spawn(move || {
            let dlg = rfd::FileDialog::new()
                .add_filter("screen", &["screen"])
                .set_file_name(file_name);
            if let Some(file) = dlg.save_file() {
                let _ = image.save(file);
            }
        });
    }

    fn on_change_device(&mut self, device: SharedString) {
        info!("on_change_device: {}", device.as_str());
        let devices = self.devices.clone();
        std::thread::spawn(move ||{
            for dev in devices{
                if device.as_str().contains(&dev.label){
                    if let Ok(mut screen) = SCREEN.lock(){
                        if screen.is_some() && screen.as_ref().unwrap().info.label == dev.label{
                            info!("已经打开屏幕:{}", dev.label);
                            return;
                        }
                        match UsbScreen::open(dev.clone()){
                            Ok(s) => {
                                screen.replace(CurrentUsbScreen { info: dev.clone(), screen: s });
                            }
                            Err(err) => {
                                error!("屏幕打开失败:{:?}", err);
                            }
                        }
                    }
                    break;
                }
            }
        });
    }

    fn on_change_fps(&mut self, fps: SharedString) {
        info!("on_change_fps {fps}");
        let fps = fps.to_string().replace("刷新率:", "").replace("帧/秒", "");
        let mut fps = fps.parse::<i32>().unwrap_or(10);
        if self.screen.width > 160 && self.screen.height > 128{
            //320x240屏幕最高不超过12帧
            if fps > 12{
                fps = 12;
            }
        }
        self.fps = fps;
        self.screen.fps = fps;
        let _ = self.screen.setup_monitor();
        let app = self.app.unwrap();
        app.set_fps(format!("刷新率:{fps}帧/秒").into());
    }

}

pub fn run() -> Result<()> {
    let app = CanvasEditor::new().unwrap();
    let mut context = CanvasEditorContext::new(app.as_weak());

    context.render_screen();

    let context = Rc::new(RefCell::new(CanvasEditorContext::new(app.as_weak())));

    //渲染回调函数, 30ms调用一次，实际渲染根据选择的帧率渲染
    let context_clone = context.clone();
    let timer = Timer::default();
    timer.start(
        TimerMode::Repeated,
        std::time::Duration::from_millis(40),
        move || {
            context_clone.borrow_mut().render_screen();
        },
    );

    //2秒钟刷新设备列表
    let context_clone = context.clone();
    let timer = Timer::default();
    timer.start(
        TimerMode::Repeated,
        std::time::Duration::from_secs(2),
        move || {
            context_clone.borrow_mut().update_device_list();
        }
    );

    // 定时刷新列表文字
    let context_clone = context.clone();
    let timer = Timer::default();
    timer.start(
        TimerMode::Repeated,
        std::time::Duration::from_secs(1),
        move || {
            context_clone.borrow_mut().refresh_model_text();
        },
    );

    let context_clone = context.clone();
    app.on_mouse_click(move |mouse_x, mouse_y, image_width, image_height| {
        context_clone
            .borrow_mut()
            .on_mouse_click(mouse_x, mouse_y, image_width, image_height);
    });

    let context_clone = context.clone();
    app.on_mouse_move(
        move |mouse_x, mouse_y, image_width, image_height, pressed: bool| {
            context_clone.borrow_mut().on_mouse_move(
                mouse_x,
                mouse_y,
                image_width,
                image_height,
                pressed,
            );
        },
    );

    let context_clone = context.clone();
    app.on_update_widget_position(move || {
        context_clone.borrow_mut().on_update_widget_position();
    });

    let context_clone = context.clone();
    app.on_update_widget_text(move || {
        context_clone.borrow_mut().on_update_widget_text();
    });

    let context_clone = context.clone();
    app.on_update_widget_image(move || {
        context_clone.borrow_mut().on_update_widget_image();
    });

    let context_clone = context.clone();
    app.on_update_widget_image_color(move || {
        context_clone.borrow_mut().on_update_widget_image_color();
    });

    let context_clone = context.clone();
    app.on_update_widget_text_color(move || {
        context_clone.borrow_mut().on_update_widget_text_color();
    });

    let context_clone = context.clone();
    app.on_update_widget_tags(move || {
        context_clone.borrow_mut().on_update_widget_tags();
    });

    let context_clone = context.clone();
    app.on_new_image_ready(move || {
        context_clone.borrow_mut().on_new_image_ready();
    });

    let context_clone = context.clone();
    app.on_select_widget(move |uuid| {
        context_clone.borrow_mut().on_select_widget(uuid);
    });

    let context_clone = context.clone();
    app.on_move_down_widget(move |uuid| {
        //下移，即组件的索引往前移动
        context_clone.borrow_mut().move_up_widget(uuid);
    });

    let context_clone = context.clone();
    app.on_move_up_widget(move |uuid| {
        //上移，即组件的索引往后移动
        context_clone.borrow_mut().move_back_widget(uuid);
    });

    let context_clone = context.clone();
    app.on_delete_widget(move |uuid| {
        context_clone.borrow_mut().delete_widget(uuid.as_str());
    });

    let context_clone = context.clone();
    app.on_clone_widget(move |uuid| {
        context_clone.borrow_mut().clone_widget(uuid.as_str());
    });

    let context_clone = context.clone();
    app.on_screen_mouse_scroll(move |dx, dy| {
        context_clone.borrow_mut().on_screen_mouse_scroll(dx, dy);
    });

    let context_clone = context.clone();
    app.on_screen_key_event(move |event| {
        context_clone.borrow_mut().on_screen_key_event(event);
    });

    let context_clone = context.clone();
    app.on_change_screen(move |index| {
        context_clone.borrow_mut().on_change_screen(index);
    });

    let context_clone = context.clone();
    app.on_save_screen(move || {
        context_clone.borrow_mut().on_save_screen();
    });

    let context_clone = context.clone();
    app.on_save_capture(move ||{
        context_clone.borrow_mut().on_save_capture();
    });

    let context_clone = context.clone();
    app.on_open_screen(move || {
        context_clone.borrow_mut().on_open_screen();
    });

    let context_clone = context.clone();
    app.on_open_font(move || {
        context_clone.borrow_mut().on_open_font();
    });

    //选择颜色
    let context_clone = context.clone();
    app.on_color_picker_choose_color(move |x, y| {
        context_clone
            .borrow_mut()
            .on_color_picker_choose_color(x, y)
    });

    let context_clone = context.clone();
    app.on_color_picker_brightness_change(move || {
        context_clone
            .borrow_mut()
            .on_color_picker_brightness_change();
    });

    let context_clone = context.clone();
    app.on_change_device(move |device| {
        context_clone.borrow_mut().on_change_device(device);
    });

    let context_clone = context.clone();
    app.on_change_fps(move |fps| {
        context_clone.borrow_mut().on_change_fps(fps);
    });


    #[cfg(windows)]
    info!("http服务端口号:{}", *crate::monitor::HTTP_PORT);

    app.run()?;
    Ok(())
}
