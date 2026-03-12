#![cfg(windows)]

use anyhow::{anyhow, Result};
use libloading::Library;
use log::{info, warn};
use once_cell::sync::Lazy;
use serde::{de::DeserializeOwned, Deserialize};
use std::os::windows::process::CommandExt;
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::monitor::{HardwareData, HardwareInfo};

const CREATE_NO_WINDOW: u32 = 0x08000000;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProviderKind {
    Libre,
    #[cfg(feature = "openhardware")]
    Open,
}

#[cfg(feature = "openhardware")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OhmMode {
    Extended,
    Compatibility,
}

enum HardwareMonitorProvider {
    NativeAot(NativeAotProvider),
    #[cfg(feature = "openhardware")]
    OhmProcess(OhmProcessProvider),
}

struct NativeAotDefinition {
    kind: ProviderKind,
    display_name: &'static str,
    file_name: &'static str,
    embedded_bytes: &'static [u8],
    init_symbol: &'static [u8],
    update_symbol: &'static [u8],
    get_json_symbol: &'static [u8],
    get_all_sensors_json_symbol: &'static [u8],
    get_last_error_symbol: &'static [u8],
    free_buffer_symbol: &'static [u8],
    close_symbol: &'static [u8],
}

struct NativeAotProvider {
    kind: ProviderKind,
    display_name: &'static str,
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

#[cfg(feature = "openhardware")]
struct OhmProcessProvider {
    display_name: &'static str,
    process: Child,
    working_dir: PathBuf,
    port: u16,
    mode: OhmMode,
    latest_json: Arc<Mutex<Option<String>>>,
    stop_flag: Arc<AtomicBool>,
    server_thread: Option<JoinHandle<()>>,
    service_log_path: PathBuf,
    stdout_log_path: PathBuf,
    stderr_log_path: PathBuf,
    fallback_attempted: bool,
}

static LHM_DEFINITION: NativeAotDefinition = NativeAotDefinition {
    kind: ProviderKind::Libre,
    display_name: "LibreHardwareMonitor",
    file_name: "LhmNativeAotWrapper.dll",
    embedded_bytes: EMBEDDED_LHM_WRAPPER,
    init_symbol: b"lhm_init",
    update_symbol: b"lhm_update",
    get_json_symbol: b"lhm_get_json",
    get_all_sensors_json_symbol: b"lhm_get_all_sensors_json",
    get_last_error_symbol: b"lhm_get_last_error",
    free_buffer_symbol: b"lhm_free_buffer",
    close_symbol: b"lhm_close",
};

static HARDWARE_MONITOR: Lazy<Mutex<Vec<HardwareMonitorProvider>>> =
    Lazy::new(|| Mutex::new(Vec::new()));
static DLL_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn ensure_hardware_monitor_started() -> Result<()> {
    let mut guard = HARDWARE_MONITOR
        .lock()
        .map_err(|_| anyhow!("硬件监控锁已损坏"))?;

    if guard.is_empty() {
        *guard = load_hardware_providers()?;
    }

    Ok(())
}

pub fn get_hardware_data() -> Result<Option<HardwareData>> {
    ensure_hardware_monitor_started()?;

    let mut guard = HARDWARE_MONITOR
        .lock()
        .map_err(|_| anyhow!("硬件监控锁已损坏"))?;
    if guard.is_empty() {
        return Ok(None);
    }

    let mut libre_data = None;
    let mut open_data = None;

    for provider in guard.iter_mut() {
        let result = match provider {
            HardwareMonitorProvider::NativeAot(api) => api.update_and_read_json::<HardwareData>(),
            #[cfg(feature = "openhardware")]
            HardwareMonitorProvider::OhmProcess(api) => api.read_hardware_data(),
        };

        match result {
            Ok(Some(data)) => match provider.kind() {
                ProviderKind::Libre => libre_data = Some(data),
                #[cfg(feature = "openhardware")]
                ProviderKind::Open => open_data = Some(data),
            },
            Ok(None) => {}
            Err(err) => warn!("读取 {} 硬件监控数据失败:{err:?}", provider.display_name()),
        }
    }

    Ok(merge_hardware_data(libre_data, open_data))
}

pub fn clean_hardware_monitor() {
    if let Ok(mut guard) = HARDWARE_MONITOR.lock() {
        let providers = std::mem::take(&mut *guard);
        drop(guard);

        for provider in providers {
            provider.shutdown();
        }
    }
}

impl HardwareMonitorProvider {
    fn kind(&self) -> ProviderKind {
        match self {
            Self::NativeAot(api) => api.kind,
            #[cfg(feature = "openhardware")]
            Self::OhmProcess(_) => ProviderKind::Open,
        }
    }

    fn display_name(&self) -> &'static str {
        match self {
            Self::NativeAot(api) => api.display_name,
            #[cfg(feature = "openhardware")]
            Self::OhmProcess(api) => api.display_name,
        }
    }

    fn shutdown(self) {
        match self {
            Self::NativeAot(api) => api.shutdown(),
            #[cfg(feature = "openhardware")]
            Self::OhmProcess(api) => api.shutdown(),
        }
    }
}

impl NativeAotProvider {
    fn load(definition: &NativeAotDefinition) -> Result<Self> {
        let dll_path = extract_embedded_dll(definition.file_name, definition.embedded_bytes)?;
        let library = unsafe { Library::new(&dll_path) }?;

        let init = unsafe { *library.get::<InitFn>(definition.init_symbol)? };
        let update = unsafe { *library.get::<UpdateFn>(definition.update_symbol)? };
        let get_json = unsafe { *library.get::<GetBufferFn>(definition.get_json_symbol)? };
        let get_all_sensors_json =
            unsafe { *library.get::<GetBufferFn>(definition.get_all_sensors_json_symbol)? };
        let get_last_error =
            unsafe { *library.get::<GetBufferFn>(definition.get_last_error_symbol)? };
        let free_buffer = unsafe { *library.get::<FreeBufferFn>(definition.free_buffer_symbol)? };
        let close = unsafe { *library.get::<CloseFn>(definition.close_symbol)? };

        Ok(Self {
            kind: definition.kind,
            display_name: definition.display_name,
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

    fn ensure_ready(&mut self) -> Result<()> {
        if !self.initialized {
            let status = unsafe { (self.init)() };
            check_native_status(self, status)?;
            self.initialized = true;
        }

        if !self.sensors_dumped {
            if let Err(err) = dump_all_sensors(self) {
                warn!("打印 {} 全量传感器失败:{err:?}", self.display_name);
            }
            self.sensors_dumped = true;
        }

        Ok(())
    }

    fn update_and_read_json<T>(&mut self) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let status = unsafe { (self.update)() };
        check_native_status(self, status)?;
        read_native_json_data(self, unsafe { (self.get_json)() })
    }

    fn shutdown(self) {
        let NativeAotProvider {
            kind,
            _library: library,
            dll_path,
            initialized,
            close,
            ..
        } = self;

        if matches!(kind, ProviderKind::Libre) {
            // LHM NativeAOT 在 Windows 退出阶段可能触发 HidSharp 后台线程异常
            // 这里不主动 close 或卸载库 直接让进程退出时由 OS 回收
            std::mem::forget(library);
            std::mem::forget(dll_path);
            return;
        }

        if initialized {
            unsafe {
                (close)();
            }
        }

        drop(library);
        let _ = fs::remove_file(&dll_path);
    }
}

#[cfg(feature = "openhardware")]
impl OhmProcessProvider {
    fn start() -> Result<Self> {
        let working_dir = extract_ohm_process_files()?;
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;

        let latest_json = Arc::new(Mutex::new(None));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let server_thread = start_ohm_http_server(
            listener,
            Arc::clone(&latest_json),
            Arc::clone(&stop_flag),
        );

        let port = address.port();
        let service_log_path = working_dir.join("OpenHardwareMonitorService.log");
        let (process, stdout_log_path, stderr_log_path) =
            spawn_ohm_process(&working_dir, port, OhmMode::Extended)?;

        Ok(Self {
            display_name: "OpenHardwareMonitor",
            process,
            working_dir,
            port,
            mode: OhmMode::Extended,
            latest_json,
            stop_flag,
            server_thread: Some(server_thread),
            service_log_path,
            stdout_log_path,
            stderr_log_path,
            fallback_attempted: false,
        })
    }

    fn read_hardware_data(&mut self) -> Result<Option<HardwareData>> {
        self.ensure_running()?;
        let json = self
            .latest_json
            .lock()
            .map_err(|_| anyhow!("OHM 子进程缓存锁已损坏"))?
            .clone();

        match json {
            Some(json) if !json.is_empty() => Ok(Some(serde_json::from_str::<HardwareData>(&json)?)),
            _ => Ok(None),
        }
    }

    fn ensure_running(&mut self) -> Result<()> {
        if let Some(status) = self.process.try_wait()? {
            let exit_code = status.code().unwrap_or(-1);
            let log_tail = read_ohm_process_log_tail(
                &self.service_log_path,
                &self.stdout_log_path,
                &self.stderr_log_path,
            );

            if !self.fallback_attempted && self.mode == OhmMode::Extended {
                warn!(
                    "OpenHardwareMonitor 扩展模式已退出 exit_code={}{}",
                    exit_code,
                    format_log_tail(&log_tail)
                );
                self.restart_in_mode(OhmMode::Compatibility)?;
                info!("OpenHardwareMonitor 子进程已自动切换到兼容模式");
                return Ok(());
            }

            Err(anyhow!(
                "OpenHardwareMonitorService 已退出 exit_code={}{}",
                exit_code,
                format_log_tail(&log_tail)
            ))
        } else {
            Ok(())
        }
    }

    fn restart_in_mode(&mut self, mode: OhmMode) -> Result<()> {
        if let Ok(mut guard) = self.latest_json.lock() {
            *guard = None;
        }

        let (process, stdout_log_path, stderr_log_path) =
            spawn_ohm_process(&self.working_dir, self.port, mode)?;
        self.process = process;
        self.mode = mode;
        self.stdout_log_path = stdout_log_path;
        self.stderr_log_path = stderr_log_path;
        self.fallback_attempted = true;
        Ok(())
    }

    fn shutdown(mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);

        let _ = TcpStream::connect(("127.0.0.1", self.port));

        if let Some(handle) = self.server_thread.take() {
            let _ = handle.join();
        }

        match self.process.try_wait() {
            Ok(Some(_)) => {}
            _ => {
                let _ = self.process.kill();
                let _ = self.process.wait();
            }
        }

        let _ = fs::remove_dir_all(&self.working_dir);
    }
}

fn load_hardware_providers() -> Result<Vec<HardwareMonitorProvider>> {
    let mut providers = Vec::new();
    let mut errors = Vec::new();

    match NativeAotProvider::load(&LHM_DEFINITION) {
        Ok(mut api) => match api.ensure_ready() {
            Ok(()) => {
                info!("{} NativeAOT 硬件监控已加载", api.display_name);
                providers.push(HardwareMonitorProvider::NativeAot(api));
            }
            Err(err) => {
                warn!("初始化 {} NativeAOT 硬件监控失败:{err:?}", LHM_DEFINITION.display_name);
                errors.push(format!("{}: {err}", LHM_DEFINITION.display_name));
                api.shutdown();
            }
        },
        Err(err) => {
            warn!("加载 {} NativeAOT 动态库失败:{err:?}", LHM_DEFINITION.display_name);
            errors.push(format!("{}: {err}", LHM_DEFINITION.display_name));
        }
    }

    #[cfg(feature = "openhardware")]
    match OhmProcessProvider::start() {
        Ok(api) => {
            info!("{} 子进程补充服务已加载", api.display_name);
            providers.push(HardwareMonitorProvider::OhmProcess(api));
        }
        Err(err) => {
            warn!("初始化 OpenHardwareMonitor 子进程补充服务失败:{err:?}");
            errors.push(format!("OpenHardwareMonitor: {err}"));
        }
    }

    #[cfg(not(feature = "openhardware"))]
    info!("OpenHardwareMonitor feature 未启用 当前仅使用 LibreHardwareMonitor NativeAOT");

    if providers.is_empty() {
        if errors.is_empty() {
            Err(anyhow!("没有可用的硬件监控提供者"))
        } else {
            Err(anyhow!("硬件监控初始化失败 {}", errors.join(" | ")))
        }
    } else {
        Ok(providers)
    }
}

fn extract_embedded_dll(file_name: &str, embedded_bytes: &[u8]) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join("usb-screen-native-aot");
    fs::create_dir_all(&dir)?;

    let unique_id = DLL_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let path = dir.join(format!(
        "{}-{}-{}.dll",
        file_name.trim_end_matches(".dll"),
        std::process::id(),
        timestamp + unique_id as u128
    ));
    fs::write(&path, embedded_bytes)?;
    Ok(path)
}

#[cfg(feature = "openhardware")]
fn extract_ohm_process_files() -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("usb-screen-ohm-process-{}", std::process::id()));
    fs::create_dir_all(&dir)?;

    fs::write(dir.join("OpenHardwareMonitorService.exe"), EMBEDDED_OHM_SERVICE_EXE)?;
    fs::write(
        dir.join("OpenHardwareMonitorService.exe.config"),
        EMBEDDED_OHM_SERVICE_CONFIG,
    )?;
    fs::write(
        dir.join("OpenHardwareMonitorLib.dll"),
        EMBEDDED_OHM_OPENHARDWAREMONITOR_LIB,
    )?;
    fs::write(dir.join("Newtonsoft.Json.dll"), EMBEDDED_OHM_NEWTONSOFT_JSON)?;

    Ok(dir)
}

#[cfg(feature = "openhardware")]
fn spawn_ohm_process(
    working_dir: &PathBuf,
    port: u16,
    mode: OhmMode,
) -> Result<(Child, PathBuf, PathBuf)> {
    let exe_path = working_dir.join("OpenHardwareMonitorService.exe");
    let stdout_log_path = working_dir.join("OpenHardwareMonitorService.stdout.log");
    let stderr_log_path = working_dir.join("OpenHardwareMonitorService.stderr.log");
    let stdout_file = File::create(&stdout_log_path)?;
    let stderr_file = File::create(&stderr_log_path)?;

    let mut command = Command::new(&exe_path);
    command
        .arg(port.to_string())
        .current_dir(working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .creation_flags(CREATE_NO_WINDOW);

    if mode == OhmMode::Compatibility {
        command.arg("compat");
    }

    let process = command
        .spawn()
        .map_err(|err| anyhow!("启动 OpenHardwareMonitorService 失败:{err}"))?;

    Ok((process, stdout_log_path, stderr_log_path))
}

#[cfg(feature = "openhardware")]
fn read_ohm_process_log_tail(
    service_log_path: &PathBuf,
    stdout_log_path: &PathBuf,
    stderr_log_path: &PathBuf,
) -> Option<String> {
    let service_log_tail = read_log_tail(service_log_path);
    if !service_log_tail.is_empty() {
        return Some(service_log_tail);
    }

    let stderr_tail = read_log_tail(stderr_log_path);
    if !stderr_tail.is_empty() {
        return Some(stderr_tail);
    }

    let stdout_tail = read_log_tail(stdout_log_path);
    if stdout_tail.is_empty() {
        None
    } else {
        Some(stdout_tail)
    }
}

#[cfg(feature = "openhardware")]
fn read_log_tail(path: &PathBuf) -> String {
    match fs::read_to_string(path) {
        Ok(content) => content
            .lines()
            .rev()
            .take(12)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(" | "),
        Err(_) => String::new(),
    }
}

#[cfg(feature = "openhardware")]
fn format_log_tail(log_tail: &Option<String>) -> String {
    match log_tail {
        Some(text) if !text.trim().is_empty() => format!(" detail={text}"),
        _ => String::new(),
    }
}

fn check_native_status(api: &NativeAotProvider, status: i32) -> Result<()> {
    if status == 0 {
        return Ok(());
    }

    let message = read_native_buffer(api, unsafe { (api.get_last_error)() });
    if message.is_empty() {
        Err(anyhow!("{} 硬件监控调用失败 status={status}", api.display_name))
    } else {
        Err(anyhow!(message))
    }
}

fn read_native_json_data<T>(api: &NativeAotProvider, buffer: NativeBuffer) -> Result<Option<T>>
where
    T: DeserializeOwned,
{
    let json = read_native_buffer(api, buffer);
    if json.is_empty() {
        return Ok(None);
    }

    Ok(Some(serde_json::from_str::<T>(&json)?))
}

fn read_native_buffer(api: &NativeAotProvider, buffer: NativeBuffer) -> String {
    if buffer.ptr.is_null() || buffer.len == 0 {
        return String::new();
    }

    let bytes = unsafe { std::slice::from_raw_parts(buffer.ptr as *const u8, buffer.len) }.to_vec();
    unsafe {
        (api.free_buffer)(buffer.ptr, buffer.len);
    }

    String::from_utf8(bytes).unwrap_or_else(|_| "invalid utf8 error message".to_string())
}

fn dump_all_sensors(api: &NativeAotProvider) -> Result<()> {
    let data = match read_native_json_data::<AllSensorData>(api, unsafe { (api.get_all_sensors_json)() })? {
        Some(data) => data,
        None => return Ok(()),
    };

    info!("========== {} 全量传感器开始 ==========", api.display_name);
    for sensor in data.sensors {
        info!(
            "provider={} hardware_type={} hardware_name={} hardware_identifier={} sensor_type={} sensor_name={} sensor_identifier={} value={}",
            api.display_name,
            sensor.hardware_type,
            sensor.hardware_name,
            sensor.hardware_identifier,
            sensor.sensor_type,
            sensor.sensor_name,
            sensor.sensor_identifier,
            sensor.value
        );
    }
    info!("========== {} 全量传感器结束 ==========", api.display_name);

    Ok(())
}

#[cfg(feature = "openhardware")]
fn start_ohm_http_server(
    listener: TcpListener,
    latest_json: Arc<Mutex<Option<String>>>,
    stop_flag: Arc<AtomicBool>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        while !stop_flag.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    if let Err(err) = handle_ohm_request(stream, &latest_json) {
                        if err
                            .downcast_ref::<std::io::Error>()
                            .map(|io_err| io_err.kind() == std::io::ErrorKind::WouldBlock)
                            .unwrap_or(false)
                        {
                            continue;
                        }
                        warn!("处理 OpenHardwareMonitor 子进程请求失败:{err:?}");
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(err) => {
                    warn!("OpenHardwareMonitor 子进程监听失败:{err:?}");
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    })
}

#[cfg(feature = "openhardware")]
fn handle_ohm_request(mut stream: TcpStream, latest_json: &Arc<Mutex<Option<String>>>) -> Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;

    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    if request_line.is_empty() {
        return Ok(());
    }

    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line == "\r\n" || line.is_empty() {
            break;
        }

        let line_lower = line.to_ascii_lowercase();
        if let Some(value) = line_lower.strip_prefix("content-length:") {
            content_length = value.trim().parse::<usize>().unwrap_or(0);
        }
    }

    if request_line.starts_with("GET /isOpen ") {
        write_http_response(&mut stream, 200, "true")?;
        return Ok(());
    }

    if request_line.starts_with("POST /upload ") {
        let mut body = vec![0u8; content_length];
        if content_length > 0 {
            reader.read_exact(&mut body)?;
        }

        let json = String::from_utf8(body).unwrap_or_default();
        if !json.is_empty() {
            if let Ok(mut guard) = latest_json.lock() {
                *guard = Some(json);
            }
        }

        write_http_response(&mut stream, 200, "ok")?;
        return Ok(());
    }

    write_http_response(&mut stream, 404, "not found")?;
    Ok(())
}

#[cfg(feature = "openhardware")]
fn write_http_response(stream: &mut TcpStream, status_code: u16, body: &str) -> Result<()> {
    let status_text = match status_code {
        200 => "OK",
        404 => "Not Found",
        _ => "Error",
    };
    let body_bytes = body.as_bytes();
    let response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status_code,
        status_text,
        body_bytes.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.write_all(body_bytes)?;
    stream.flush()?;
    Ok(())
}

fn merge_hardware_data(primary: Option<HardwareData>, fallback: Option<HardwareData>) -> Option<HardwareData> {
    match (primary, fallback) {
        (Some(primary), Some(fallback)) => Some(HardwareData {
            cpu_infos: merge_hardware_info_vec(primary.cpu_infos, fallback.cpu_infos),
            gpu_infos: merge_hardware_info_vec(primary.gpu_infos, fallback.gpu_infos),
        }),
        (Some(primary), None) => Some(primary),
        (None, Some(fallback)) => Some(fallback),
        (None, None) => None,
    }
}

fn merge_hardware_info_vec(primary: Vec<HardwareInfo>, fallback: Vec<HardwareInfo>) -> Vec<HardwareInfo> {
    let mut merged = Vec::new();
    let mut primary_iter = primary.into_iter();
    let mut fallback_iter = fallback.into_iter();

    loop {
        match (primary_iter.next(), fallback_iter.next()) {
            (Some(primary_info), Some(fallback_info)) => {
                merged.push(merge_hardware_info(primary_info, fallback_info));
            }
            (Some(primary_info), None) => {
                merged.push(primary_info);
                merged.extend(primary_iter);
                break;
            }
            (None, Some(fallback_info)) => {
                merged.push(fallback_info);
                merged.extend(fallback_iter);
                break;
            }
            (None, None) => break,
        }
    }

    merged
}

fn merge_hardware_info(mut primary: HardwareInfo, fallback: HardwareInfo) -> HardwareInfo {
    if primary.fans.is_empty() {
        primary.fans = fallback.fans;
    }
    if primary.temperatures.is_empty() {
        primary.temperatures = fallback.temperatures;
    }
    if primary.loads.is_empty() {
        primary.loads = fallback.loads;
    }
    if primary.clocks.is_empty() {
        primary.clocks = fallback.clocks;
    }
    if is_missing_scalar(primary.package_power) {
        primary.package_power = fallback.package_power;
    }
    if is_missing_scalar(primary.cores_power) {
        primary.cores_power = fallback.cores_power;
    }
    if is_missing_scalar(primary.total_load) {
        primary.total_load = fallback.total_load;
    }
    if is_missing_scalar(primary.total_temperature) {
        primary.total_temperature = fallback.total_temperature;
    }
    if is_missing_scalar(primary.memory_load) {
        primary.memory_load = fallback.memory_load;
    }
    if is_missing_scalar(primary.memory_total) {
        primary.memory_total = fallback.memory_total;
    }

    primary
}

fn is_missing_scalar(value: f32) -> bool {
    !value.is_finite() || value <= 0.0
}
