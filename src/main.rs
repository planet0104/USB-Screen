#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{path::Path, process::Command, time::{Duration, Instant}};

use anyhow::{anyhow, Result};
use image::{buffer::ConvertBuffer, RgbImage};
use log::{error, info};
#[cfg(feature = "tray")]
use tao::event_loop::ControlFlow;

use usb_screen::find_and_open_a_screen;

use crate::screen::ScreenRender;
#[cfg(feature = "editor")]
mod editor;
mod monitor;
mod nmc;
mod rgb565;
mod screen;
mod usb_screen;
mod wifi_screen;
mod utils;
mod widgets;
#[cfg(all(not(windows),feature = "v4l-webcam"))]
mod yuv422;

fn main() -> Result<()> {
    // env_logger::init();
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .try_init();

    #[cfg(windows)]
    {
        #[cfg(not(debug_assertions))]
        {
            let exe_path = std::env::current_exe()?;
            std::env::set_current_dir(exe_path.parent().unwrap())?;
        }
    }

    let args: Vec<String> = std::env::args().skip(1).collect();

    let screen_file = match args.len() {
        0 => read_screen_file(),
        1 => Some(args[0].to_string()),
        _ => None,
    };

    info!("screen_file={:?}", screen_file);

    if let Some(file) = screen_file {
        #[cfg(feature = "editor")]
        if file != "editor"{
            create_tray_icon(file)?;
            return Ok(());
        }

        #[cfg(not(feature = "editor"))]
        create_tray_icon(file)?;
    }

    #[cfg(feature = "editor")]
    {
        info!("editor start!");
        editor::run()?;
        monitor::clean();
    }
    Ok(())
}

fn open_usb_screen(file: String) -> Result<()>{
    let f = std::fs::read(file)?;
    let mut render = ScreenRender::new_from_file(&f)?;

    render.setup_monitor()?;
    let mut usb_screen = usb_screen::find_and_open_a_screen();
    info!("USB Screen是否已打开: {}", usb_screen.is_some());
    let mut last_draw_time = Instant::now();
    let frame_duration = (1000./render.fps) as u128;
    info!("帧时间:{}ms", frame_duration);
    //设置系统信息更新延迟
    let _ = monitor::set_update_delay(frame_duration);
    loop {
        if last_draw_time.elapsed().as_millis() < frame_duration{
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        last_draw_time = Instant::now();
        render.render();
        let frame: RgbImage = render.canvas.image_data().convert();
        //旋转
        let frame = if render.rotate_degree == 90 {
            image::imageops::rotate90(&frame)
        }else if render.rotate_degree == 180{
            image::imageops::rotate180(&frame)
        }else if render.rotate_degree == 270{
            image::imageops::rotate270(&frame)
        }else{
            frame
        };
        // let rgb565 = rgb888_to_rgb565_u16(&frame, frame.width() as usize, frame.height() as usize);
        if usb_screen.is_none() {
            std::thread::sleep(Duration::from_millis(2000));
            info!("open USB Screen...");
            usb_screen = find_and_open_a_screen();
        } else {
            let screen = usb_screen.as_mut().unwrap();
            if let Err(err) = screen.draw_rgb_image(
                0,
                0,
                &frame
            )
            {
                error!("屏幕绘制失败:{err:?}");
                usb_screen = None;
            }
        }
    }
}

fn create_tray_icon(file: String) -> Result<()> {

    #[cfg(not(feature = "editor"))]
    {
        let ret = open_usb_screen(file);
        error!("{:?}", ret);
        return Ok(());
    }

    #[cfg(feature = "tray")]
    {
        std::thread::spawn(move ||{
            let ret = open_usb_screen(file);
            error!("{:?}", ret);
        });
    
        // 图标必须运行在UI线程上
        let event_loop = tao::event_loop::EventLoopBuilder::new().build();
    
        let tray_menu = Box::new(tray_icon::menu::Menu::new());
        let quit_i = tray_icon::menu::MenuItem::new("退出", true, None);
        let editor_i = tray_icon::menu::MenuItem::new("编辑器", true, None);
        let _ = tray_menu.append(&quit_i);
        let _ = tray_menu.append(&editor_i);
        let mut tray_icon = None;
        let mut menu_channel = None;
    
        event_loop.run(move |event, _, control_flow| {
            // We add delay of 16 ms (60fps) to event_loop to reduce cpu load.
            // This can be removed to allow ControlFlow::Poll to poll on each cpu cycle
            // Alternatively, you can set ControlFlow::Wait or use TrayIconEvent::set_event_handler,
            // see https://github.com/tauri-apps/tray-icon/issues/83#issuecomment-1697773065
            *control_flow = ControlFlow::WaitUntil(
                std::time::Instant::now() + std::time::Duration::from_millis(16),
            );
    
            if let tao::event::Event::NewEvents(tao::event::StartCause::Init) = event {
                //创建图标
                let icon = image::load_from_memory(include_bytes!("../images/monitor.png")).unwrap().to_rgba8();
                let (width, height) = icon.dimensions();
                
                
                if let Ok(icon) = tray_icon::Icon::from_rgba(icon.into_raw(), width, height){
                    if let Ok(i) = tray_icon::TrayIconBuilder::new()
                    .with_tooltip("USB Screen")
                    .with_menu(tray_menu.clone())
                    .with_icon(icon)
                    .build(){
                        tray_icon = Some(i);
                        menu_channel = Some(tray_icon::menu::MenuEvent::receiver());
                    }
                }
    
                // We have to request a redraw here to have the icon actually show up.
                // Tao only exposes a redraw method on the Window so we use core-foundation directly.
                #[cfg(target_os = "macos")]
                unsafe {
                    use core_foundation::runloop::{CFRunLoopGetMain, CFRunLoopWakeUp};
    
                    let rl = CFRunLoopGetMain();
                    CFRunLoopWakeUp(rl);
                }
            }
    
            if let (Some(_tray_icon), Some(menu_channel)) = (tray_icon.as_mut(), menu_channel.as_mut()){
                if let Ok(event) = menu_channel.try_recv() {
                    if event.id == quit_i.id() {
                        *control_flow = ControlFlow::Exit;
                    }else if event.id == editor_i.id() {
                        //启动自身
                        if let Ok(_) = run_as_editor(){
                            //退出托盘
                            *control_flow = ControlFlow::Exit;
                        }
                    }
                }
            }
        });
    }
    Ok(())
}

fn read_screen_file() -> Option<String> {
    // #[cfg(debug_assertions)]
    // {
    //     return None;
    // }
    //在当前目录下查找.screen文件
    let path = Path::new("./"); // 这里以当前目录为例，你可以替换为任何你想要列出的目录路径
                                // 使用read_dir函数读取目录条目
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.is_file() {
                    if let Some(extension) = path.extension() {
                        if extension == "screen" {
                            if let Some(str) = path.to_str() {
                                return Some(str.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

#[cfg(windows)]
pub fn is_run_as_admin() -> Result<bool> {
    use std::mem::MaybeUninit;
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    unsafe {
        let mut token_handle: HANDLE = HANDLE(std::ptr::null_mut());
        let process_handle = GetCurrentProcess();

        // 打开进程令牌
        OpenProcessToken(process_handle, TOKEN_QUERY, &mut token_handle)?;
        if token_handle.is_invalid() {
            return Ok(false);
        }

        // 获取令牌信息
        let mut elevation_buffer_size: u32 = 0;
        let mut elevation_info: MaybeUninit<TOKEN_ELEVATION> = MaybeUninit::uninit();
        let elevation_info_ptr = elevation_info.as_mut_ptr() as *mut _;
        let expect_size = std::mem::size_of::<TOKEN_ELEVATION>() as u32;
        GetTokenInformation(
            token_handle,
            TokenElevation,
            Some(elevation_info_ptr),
            expect_size,
            &mut elevation_buffer_size,
        )?;
        // 检查 TokenIsElevated 标志
        let elevation = elevation_info.assume_init();
        let is_elevated = elevation.TokenIsElevated != 0;
        // 关闭令牌句柄
        CloseHandle(token_handle)?;
        return Ok(is_elevated);
    }
}

#[cfg(windows)]
pub fn run_as_admin(params: Option<&str>) -> Result<()> {
    use anyhow::anyhow;
    use windows::{
        core::{s, PCSTR},
        Win32::{
            Foundation::{HANDLE, HINSTANCE, HWND},
            System::Registry::HKEY,
            UI::Shell::{
                ShellExecuteExA, SEE_MASK_DOENVSUBST, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOCLOSEPROCESS,
                SHELLEXECUTEINFOA, SHELLEXECUTEINFOA_0,
            },
        },
    };

    let exe_path = std::env::current_exe()?;
    let exe_path = exe_path.to_str();
    if exe_path.is_none() {
        return Err(anyhow!("exe path error!"));
    }
    let mut exe_path = exe_path.unwrap().to_string();
    exe_path.push('\0');

    let params_ptr = if let Some(s) = params {
        let mut s = s.to_string();
        s.push('\n');
        PCSTR::from_raw(s.as_ptr())
    } else {
        PCSTR::from_raw(std::ptr::null())
    };

    info!("Executable path: {exe_path}");
    unsafe {
        let mut sh_exec_info = SHELLEXECUTEINFOA {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOA>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_DOENVSUBST | SEE_MASK_FLAG_NO_UI,
            hwnd: HWND(std::ptr::null_mut()),
            lpVerb: s!("runas"),
            lpFile: PCSTR::from_raw(exe_path.as_ptr()),
            lpParameters: params_ptr,
            lpDirectory: PCSTR::null(),
            nShow: 0,
            hInstApp: HINSTANCE(std::ptr::null_mut()),
            lpIDList: std::ptr::null_mut(),
            lpClass: PCSTR::null(),
            hkeyClass: HKEY(std::ptr::null_mut()),
            dwHotKey: 0,
            hProcess: HANDLE(std::ptr::null_mut()),
            Anonymous: SHELLEXECUTEINFOA_0::default(),
        };

        ShellExecuteExA(&mut sh_exec_info)?;
    }
    Ok(())
}


pub fn run_as_editor() -> Result<()> {
    let exe_path = std::env::current_exe()?;
    let exe_path = exe_path.to_str();
    if exe_path.is_none() {
        return Err(anyhow!("exe path error!"));
    }
    let mut command = Command::new(exe_path.unwrap());
    command.arg("editor");
    command.spawn()?;
    Ok(())
}
