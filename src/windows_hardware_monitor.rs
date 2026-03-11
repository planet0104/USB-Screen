#![cfg(windows)]

use anyhow::{anyhow, Result};
use libloading::Library;
use log::{info, warn};
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::{
    fs,
    path::PathBuf,
    sync::Mutex,
};

include!(concat!(env!("OUT_DIR"), "/lhm_wrapper_bytes.rs"));

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NativeBuffer {
    ptr: *mut u8,
    len: usize,
}

type InitFn = unsafe extern "C" fn() -> i32;
type UpdateFn = unsafe extern "C" fn() -> i32;
type GetBufferFn = unsafe extern "C" fn() -> NativeBuffer;
type FreeBufferFn = unsafe extern "C" fn(*mut u8, usize);
type CloseFn = unsafe extern "C" fn();

#[derive(Debug, Deserialize)]
struct AllSensorData {
    sensors: Vec<SensorInfo>,
}

#[derive(Debug, Deserialize)]
struct SensorInfo {
    hardware_type: String,
    hardware_name: String,
    hardware_identifier: String,
    sensor_type: String,
    sensor_name: String,
    sensor_identifier: String,
    value: f32,
}

struct HardwareMonitorApi {
    _library: Library,
    dll_path: PathBuf,
    initialized: bool,
    sensors_dumped: bool,
    init: InitFn,
    update: UpdateFn,
    get_json: GetBufferFn,
    get_all_sensors_json: GetBufferFn,
    get_last_error: GetBufferFn,
    free_buffer: FreeBufferFn,
    close: CloseFn,
}

static HARDWARE_MONITOR: Lazy<Mutex<Option<HardwareMonitorApi>>> = Lazy::new(|| Mutex::new(None));

pub fn ensure_hardware_monitor_started() -> Result<()> {
    let mut guard = HARDWARE_MONITOR
        .lock()
        .map_err(|_| anyhow!("硬件监控锁已损坏"))?;

    if guard.is_none() {
        *guard = Some(HardwareMonitorApi::load()?);
    }

    let api = guard
        .as_mut()
        .ok_or_else(|| anyhow!("硬件监控初始化失败"))?;

    if !api.initialized {
        let status = unsafe { (api.init)() };
        check_status(api, status)?;
        api.initialized = true;
    }

    if !api.sensors_dumped {
        if let Err(err) = dump_all_sensors(api) {
            warn!("打印 LibreHardwareMonitor 全量传感器失败:{err:?}");
        }
        api.sensors_dumped = true;
    }

    Ok(())
}

pub fn get_hardware_data() -> Result<Option<crate::monitor::HardwareData>> {
    ensure_hardware_monitor_started()?;

    let mut guard = HARDWARE_MONITOR
        .lock()
        .map_err(|_| anyhow!("硬件监控锁已损坏"))?;
    let api = guard
        .as_mut()
        .ok_or_else(|| anyhow!("硬件监控未就绪"))?;

    let status = unsafe { (api.update)() };
    check_status(api, status)?;

    let buffer = unsafe { (api.get_json)() };
    if buffer.ptr.is_null() || buffer.len == 0 {
        return Ok(None);
    }

    let bytes = unsafe { std::slice::from_raw_parts(buffer.ptr as *const u8, buffer.len) }.to_vec();
    unsafe {
        (api.free_buffer)(buffer.ptr, buffer.len);
    }

    let data = serde_json::from_slice::<crate::monitor::HardwareData>(&bytes)?;
    Ok(Some(data))
}

pub fn clean_hardware_monitor() {
    if let Ok(mut guard) = HARDWARE_MONITOR.lock() {
        if let Some(api) = guard.take() {
            let HardwareMonitorApi {
                _library: library,
                dll_path,
                initialized,
                close,
                ..
            } = api;

            if initialized {
                unsafe {
                    (close)();
                }
            }

            drop(library);
            let _ = fs::remove_file(&dll_path);
        }
    }
}

impl HardwareMonitorApi {
    fn load() -> Result<Self> {
        let dll_path = extract_embedded_wrapper()?;
        let library = unsafe { Library::new(&dll_path) }?;

        let init = unsafe { *library.get::<InitFn>(b"lhm_init")? };
        let update = unsafe { *library.get::<UpdateFn>(b"lhm_update")? };
        let get_json = unsafe { *library.get::<GetBufferFn>(b"lhm_get_json")? };
        let get_all_sensors_json =
            unsafe { *library.get::<GetBufferFn>(b"lhm_get_all_sensors_json")? };
        let get_last_error = unsafe { *library.get::<GetBufferFn>(b"lhm_get_last_error")? };
        let free_buffer = unsafe { *library.get::<FreeBufferFn>(b"lhm_free_buffer")? };
        let close = unsafe { *library.get::<CloseFn>(b"lhm_close")? };

        Ok(Self {
            _library: library,
            dll_path,
            initialized: false,
            sensors_dumped: false,
            init,
            update,
            get_json,
            get_all_sensors_json,
            get_last_error,
            free_buffer,
            close,
        })
    }
}

fn extract_embedded_wrapper() -> Result<PathBuf> {
    let dir = std::env::temp_dir().join("usb-screen-native-aot");
    fs::create_dir_all(&dir)?;

    let path = dir.join(format!("LhmNativeAotWrapper-{}.dll", std::process::id()));
    fs::write(&path, EMBEDDED_LHM_WRAPPER)?;
    Ok(path)
}

fn check_status(api: &HardwareMonitorApi, status: i32) -> Result<()> {
    if status == 0 {
        return Ok(());
    }

    let message = read_buffer(api, unsafe { (api.get_last_error)() });
    if message.is_empty() {
        Err(anyhow!("硬件监控调用失败 status={status}"))
    } else {
        Err(anyhow!(message))
    }
}

fn read_buffer(api: &HardwareMonitorApi, buffer: NativeBuffer) -> String {
    if buffer.ptr.is_null() || buffer.len == 0 {
        return String::new();
    }

    let bytes = unsafe { std::slice::from_raw_parts(buffer.ptr as *const u8, buffer.len) }.to_vec();
    unsafe {
        (api.free_buffer)(buffer.ptr, buffer.len);
    }

    String::from_utf8(bytes).unwrap_or_else(|_| "invalid utf8 error message".to_string())
}

fn dump_all_sensors(api: &HardwareMonitorApi) -> Result<()> {
    let buffer = unsafe { (api.get_all_sensors_json)() };
    let json = read_buffer(api, buffer);
    if json.is_empty() {
        return Ok(());
    }

    let data = serde_json::from_str::<AllSensorData>(&json)?;

    info!("========== LibreHardwareMonitor 全量传感器开始 ==========");
    for sensor in data.sensors {
        info!(
            "hardware_type={} hardware_name={} hardware_identifier={} sensor_type={} sensor_name={} sensor_identifier={} value={}",
            sensor.hardware_type,
            sensor.hardware_name,
            sensor.hardware_identifier,
            sensor.sensor_type,
            sensor.sensor_name,
            sensor.sensor_identifier,
            sensor.value
        );
    }
    info!("========== LibreHardwareMonitor 全量传感器结束 ==========");

    Ok(())
}
