#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{path::Path, time::Duration};

use anyhow::Result;
use image::{buffer::ConvertBuffer, RgbImage};
use log::info;
use rgb565::rgb888_to_rgb565_u16;
use usb_screen::draw_rgb565;

use crate::screen::ScreenRender;
mod editor;
mod monitor;
mod nmc;
mod rgb565;
mod screen;
mod usb_screen;
mod utils;
mod widgets;

fn main() -> Result<()> {
    // env_logger::init();
    let _ = env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .try_init();
    info!("editor start!");

    let args: Vec<String> = std::env::args().skip(1).collect();

    let screen_file = match args.len() {
        0 => read_screen_file(),
        1 => Some(args[0].to_string()),
        _ => None,
    };

    if let Some(file) = screen_file {
        let f = std::fs::read(file)?;
        let render = ScreenRender::new_from_file(&f)?;
        open_usb_screen(render)?;
        return Ok(());
    }

    editor::run()?;
    monitor::clean();
    Ok(())
}

fn open_usb_screen(mut render: ScreenRender) -> Result<()> {
    render.setup_monitor()?;
    let mut usb_screen = usb_screen::open_usb_screen("USB Screen", "62985215")?;
    loop {
        render.render();
        let frame: RgbImage = render.canvas.image_data().convert();
        let rgb565 = rgb888_to_rgb565_u16(&frame, frame.width() as usize, frame.height() as usize);
        if usb_screen.is_none() {
            std::thread::sleep(Duration::from_millis(2000));
            println!("open USB Screen...");
            usb_screen = usb_screen::open_usb_screen("USB Screen", "62985215")?;
        } else {
            let interface = usb_screen.as_mut().unwrap();
            if draw_rgb565(
                &rgb565,
                0,
                0,
                frame.width() as u16,
                frame.height() as u16,
                interface,
            )
            .is_err()
            {
                usb_screen = None;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
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
        let mut token_handle: HANDLE = HANDLE(0);
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
            hwnd: HWND(0),
            lpVerb: s!("runas"),
            lpFile: PCSTR::from_raw(exe_path.as_ptr()),
            lpParameters: params_ptr,
            lpDirectory: PCSTR::null(),
            nShow: 0,
            hInstApp: HINSTANCE(0),
            lpIDList: std::ptr::null_mut(),
            lpClass: PCSTR::null(),
            hkeyClass: HKEY(0),
            dwHotKey: 0,
            hProcess: HANDLE(0),
            Anonymous: SHELLEXECUTEINFOA_0::default(),
        };

        ShellExecuteExA(&mut sh_exec_info)?;
    }
    Ok(())
}
