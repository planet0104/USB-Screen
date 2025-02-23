use std::{net::{Ipv4Addr, TcpStream}, sync::Mutex, time::{Duration, Instant}};

use crossbeam_channel::{bounded, Receiver, Sender};
use fast_image_resize::{images::Image, Resizer};
use image::{buffer::ConvertBuffer, imageops::overlay, RgbImage, RgbaImage};
use log::info;
use once_cell::sync::Lazy;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tungstenite::{connect, stream::MaybeTlsStream, WebSocket};

use crate::rgb565::rgb888_to_rgb565_be;

#[derive(Serialize, Deserialize, Debug)]
struct DisplayConfig{
    display_type: Option<String>,
    rotated_width: u32,
    rotated_height: u32
}

pub enum Message{
    Connect(String),
    Disconnect,
    Image(RgbaImage)
}

#[derive(Debug, Clone)]
pub struct StatusInfo{
    pub ip: Option<String>,
    pub status: Status,
    pub delay_ms: u64,
}

#[derive(Debug, Clone)]
pub enum Status{
    NotConnected,
    Connected,
    ConnectFail,
    Disconnected,
    Connecting,
}

impl Status{
    pub fn name(&self) -> &str{
        match self{
            Status::NotConnected => "未连接",
            Status::Connected => "连接成功",
            Status::ConnectFail => "连接失败",
            Status::Disconnected => "连接断开",
            Status::Connecting => "正在连接",
        }
    }
}

static CONFIG: Lazy<Mutex<(StatusInfo, Sender<Message>)>> = Lazy::new(|| {
    let (sender, recv) = bounded(1);
    let _ = std::thread::spawn(move ||{
        start(recv);
    });
    Mutex::new((StatusInfo{
        ip: None,
        status: Status::NotConnected,
        delay_ms: 150,
    }, sender))
});

fn set_status(ip: Option<String>, status: Status) -> Result<()>{
    let mut config = CONFIG.lock().map_err(|err| anyhow!("{err:?}"))?;
    config.0.status = status;
    config.0.ip = ip;
    Ok(())
}

pub fn set_delay_ms(delay_ms: u64) -> Result<()>{
    let mut config = CONFIG.lock().map_err(|err| anyhow!("{err:?}"))?;
    config.0.delay_ms = delay_ms;
    Ok(())
}

pub fn send_message(msg: Message) -> Result<()>{
    let sender = {
        let config = CONFIG.lock().map_err(|err| anyhow!("{err:?}"))?;
        let s = config.1.clone();
        drop(config);
        s
    };
    sender.send(msg)?;
    Ok(())
}

pub fn try_send_message(msg: Message) -> Result<()>{
    let config = CONFIG.lock().map_err(|err| anyhow!("{err:?}"))?;
    config.1.try_send(msg)?;
    Ok(())
}

pub fn get_status() -> Result<StatusInfo>{
    let config = CONFIG.lock().map_err(|err| anyhow!("{err:?}"))?;
    Ok(config.0.clone())
}

fn get_display_config(ip: &str) -> Result<DisplayConfig>{
    //获取显示器大小
    let resp = reqwest::blocking::Client::builder()
    .timeout(Duration::from_secs(2))
    .build()?
    .get(&format!("http://{ip}/display_config"))
    .send()?
    .json::<DisplayConfig>()?;
    Ok(resp)
}

fn start(receiver: Receiver<Message>){
    let mut socket: Option<WebSocket<MaybeTlsStream<TcpStream>>> = None;
    let mut screen_ip = String::new();

    println!("启动upload线程...");

    let mut display_config = None;
    let mut connected = false;
    
    loop{
        match receiver.recv(){
            Ok(msg) => {
                match msg{
                    Message::Disconnect => {
                        screen_ip = String::new();
                        if let Ok(mut cfg) = CONFIG.lock(){
                            cfg.0.status = Status::Disconnected
                        }
                        if let Some(mut s) = socket.take(){
                            let _ = s.close(None);
                        }
                    }
                    Message::Connect(ip) => {
                        screen_ip = ip.clone();
                        if let Ok(cfg) = get_display_config(&ip){
                            display_config = Some(cfg);
                        }else{
                            eprintln!("display config获取失败!");
                        }
                        println!("接收到 serverIP...");
                        connected = connect_socket(ip, &mut socket).is_ok();
                    }
                    Message::Image(mut image) => {
                        let delay_ms = {
                            if let Ok(mut cfg) = CONFIG.try_lock(){
                                cfg.0.status = if connected{
                                    Status::Connected
                                }else{
                                    Status::Disconnected
                                };
                                let v = cfg.0.delay_ms;
                                drop(cfg);
                                v
                            }else{
                                150
                            }
                        };
                        if display_config.is_none(){
                            match get_display_config(&screen_ip){
                                Ok(cfg) => {
                                    display_config = Some(cfg);
                                }
                                Err(err) => {
                                    eprintln!("Message::Image display config获取失败!");
                                    eprintln!("err:?");
                                    std::thread::sleep(Duration::from_secs(3));
                                    let screen_ip_clone = screen_ip.clone();
                                    std::thread::spawn(move ||{
                                        let r = send_message(Message::Connect(screen_ip_clone));
                                        println!("重新连接 SetIp {r:?}...");
                                    });
                                }
                            }
                        }
                        let (dst_width, dst_height) = match display_config.as_ref(){
                            Some(c) => (c.rotated_width, c.rotated_height),
                            None => continue,
                        };
                        
                        //检查socket 是否断开

                        if let Some(s) = socket.as_mut(){
                            if s.can_write(){
                                connected = true;
                            }
                        }
                        if connected{
                            if let Some(s) = socket.as_mut(){
                                let t1 = Instant::now();
                                //压缩
                                let img = match fast_resize(&mut image, dst_width, dst_height){
                                    Ok(v) => v,
                                    Err(err) => {
                                        eprintln!("图片压缩失败:{}", err.root_cause());
                                        continue;
                                    }
                                };
                                let out = rgb888_to_rgb565_be(&img, img.width() as usize, img.height() as usize);
                                let out = lz4_flex::compress_prepend_size(&out);
                                println!("resize+转rgb565+lz4压缩:{}ms {}bytes {}x{}", t1.elapsed().as_millis(), out.len(), img.width(), img.height());

                                //发送
                                let ret1 = s.write(tungstenite::Message::Binary(out.into()));
                                let ret2 = s.flush();
                                if ret1.is_err() && ret2.is_err(){
                                    info!("ws write:{ret1:?}");
                                    info!("ws flush:{ret2:?}");
                                    connected = false;
                                    let _ = socket.take();
                                }
                                std::thread::sleep(Duration::from_millis(delay_ms));
                            }
                        }else{
                            if let Some(mut s) = socket.take(){
                                let _ = s.close(None);
                            }
                            let _ = set_status(None, Status::Disconnected);
                            //3秒后重连
                            println!("连接断开 3秒后重连:{screen_ip}");
                            if screen_ip.len() > 0{
                                std::thread::sleep(Duration::from_secs(3));
                                let screen_ip_clone = screen_ip.clone();
                                std::thread::spawn(move ||{
                                    let r = send_message(Message::Connect(screen_ip_clone));
                                    println!("重新连接 SetIp {r:?}...");
                                });
                            }
                        }
                    }
                }
            }
            Err(_err) => {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn connect_socket(ip: String, old_socket: &mut Option<WebSocket<MaybeTlsStream<TcpStream>>>) -> Result<()>{
    //关闭原有连接
    if let Some(mut s) = old_socket.take(){
        let _ = s.close(None);
    }
    let _ = set_status(Some(ip.clone()), Status::Connecting);
    let url = format!("ws://{ip}/ws");
    println!("开始连接:{url}");
    if let Ok((s, _resp)) = connect(url){
        *old_socket = Some(s);
        let ret = set_status(None, Status::Connected);
        println!("连接成功{ip}.. 设置状态:{ret:?}");
    }else{
        println!("连接失败{ip}..");
        let _ = set_status(None, Status::ConnectFail);
    }
    Ok(())
}

fn fast_resize(src: &mut RgbaImage, dst_width: u32, dst_height: u32) -> Result<RgbImage>{
    let mut dst_image = Image::new(
        dst_width,
        dst_height,
        fast_image_resize::PixelType::U8x3,
    );
    let mut src:RgbImage = src.convert();
    if src.width() != dst_width || src.height() != dst_height{
        let v = Image::from_slice_u8(src.width(), src.height(), src.as_mut(), fast_image_resize::PixelType::U8x3)?;
        let mut resizer = Resizer::new();
        resizer.resize(&v, &mut dst_image, None)?;
        Ok(RgbImage::from_raw(dst_image.width(), dst_image.height(), dst_image.buffer().to_vec()).unwrap())
    }else{
        Ok(src.convert())
    }
}

//获取wifi屏幕参数，测试是否可以连接成功
pub fn test_screen_sync(ip: String) -> Result<()>{
    let resp = reqwest::blocking::get(&format!("http://{ip}/display_config"))?
        .json::<DisplayConfig>()?;
    println!("屏幕大小:{}x{}", resp.rotated_width, resp.rotated_height);
    //显示hello
    let json = r#"[{"Rectangle":{"fill_color":"black","height":240,"width":240,"stroke_width":0,"left":0,"top":0}},{"Text":{"color":"white","size":20,"text":"Hello!","x":10,"y":15}},{"Text":{"color":"white","size":20,"text":"USB Screen","x":10,"y":40}}]"#;
    //绘制
    let _resp = reqwest::blocking::Client::new()
        .post(&format!("http://{ip}/draw_canvas"))
        .body(json.as_bytes())
        .send()?
        .text()?;
    Ok(())
}