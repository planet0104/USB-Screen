use anyhow::{anyhow, Result};
use chinese_number::{ChineseCase, ChineseCountMethod, ChineseVariant, NumberToChinese};
use chrono::{Datelike, Local};
use fast_image_resize::{images::Image, Resizer};
use human_repr::HumanDuration;
use image::{DynamicImage, RgbImage};
use log::{debug, error, info, warn};
#[cfg(feature = "nokhwa-webcam")]
use nokhwa::{pixel_format::RgbFormat, utils::{CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType, Resolution}, Camera};
use once_cell::sync::Lazy;
use rust_ephemeris::lunnar::SolorDate;
use serde::{Deserialize, Serialize};

use std::{
    collections::HashMap, process::Child, sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard}, time::{Duration, Instant, SystemTime}
};
use sysinfo::Networks;

use crate::nmc::{query_weather, City, RealWeather};

const UPDATE_WEATHER_DELAY: u128 = 1000 * 60 * 5;
const UPDATE_NET_IP_DELAY: u128 = 1000 * 60 * 5;
pub const EMPTY_STRING: &str = "N/A";

#[cfg(windows)]
const OHMS_EXE_FILE: &[u8] =
    include_bytes!("../OpenHardwareMonitorService/bin/Release/OpenHardwareMonitorService.exe");

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SystemUptime {
    pub days: u32,
    pub hours: u32,
    pub minutes: u32,
    pub seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetIpInfo {
    pub country: String,
    #[serde(rename = "regionName")]
    pub region_name: String,
    pub city: String,
    pub query: String,
}

#[cfg(windows)]
#[derive(Debug, Serialize, Deserialize)]
pub struct HardwareInfo {
    pub fans: Vec<f32>,
    pub temperatures: Vec<f32>,
    pub loads: Vec<f32>,
    pub clocks: Vec<f32>,
    pub package_power: f32,
    pub cores_power: f32,
    pub total_load: f32,
    pub total_temperature: f32,
    pub memory_load: f32,
    pub memory_total: f32,
}

#[cfg(windows)]
#[derive(Debug, Serialize, Deserialize)]
pub struct HardwareData {
    pub cpu_infos: Vec<HardwareInfo>,
    pub gpu_infos: Vec<HardwareInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebcamInfo{
    pub index: u32,
    pub fps: u32,
    pub width: u32,
    pub height: u32
}

pub struct SystemInfo {
    update_delay: u128,
    watch_memory: bool,
    watch_disk: bool,
    watch_disk_speed: bool,
    watch_cpu: bool,
    watch_cpu_clock_speed: bool,
    watch_cpu_temperatures: bool,
    watch_cpu_power: bool,
    watch_cpu_fan: bool,
    watch_gpu_clock_speed: bool,
    watch_gpu_temperatures: bool,
    watch_gpu_fan: bool,
    watch_gpu_load: bool,
    watch_process: bool,
    watch_weather: Option<City>,
    watch_network_speed: bool,
    watch_net_ip: bool,

    memory_info: String,
    memory_percent: String,
    swap_info: String,
    swap_percent: String,
    num_cpus: String,
    cpu_brand: String,
    cpu_usage_percpu: HashMap<usize, String>,
    cpu_usage: String,
    cpu_clock_speed: Vec<f32>,
    cpu_temperatures: Vec<f32>,
    cpu_temperature_total: f32,
    cpu_package_power: f32,
    cpu_cores_power: f32,
    cpu_fans: Vec<f32>,
    gpu_clocks: Vec<Vec<f32>>,
    gpu_temperatures: Vec<Vec<f32>>,
    gpu_temperature_total: Vec<f32>,
    gpu_package_power: f32,
    gpu_cores_power: f32,
    gpu_fans: Vec<Vec<f32>>,
    gpu_load: Vec<Vec<f32>>,
    gpu_memory_load: Vec<f32>,
    gpu_memory_total: Vec<f32>,
    gpu_load_total: Vec<f32>,
    num_process: String,
    disk_usage: HashMap<usize, String>,
    disk_speed_per_sec: (String, String),
    network_speed_per_sec: (String, String),
    system_name: String,
    kernel_version: String,
    os_version: String,
    host_name: String,
    local_ip: String,
    net_ip: Option<NetIpInfo>,
    weather_info: Option<RealWeather>,
    cpu_freq_query_task: Option<std::thread::JoinHandle<()>>,
    watch_disk_speed_task: Option<std::thread::JoinHandle<()>>,
    watch_network_speed_task: Option<std::thread::JoinHandle<()>>,
    watch_webcam_task: Option<std::thread::JoinHandle<()>>,
    hardware_monitor_service: Option<Child>,
    //缓存最新的相机图像
    webcam_frame: Option<RgbImage>,
    //监控的相机编号以及帧率
    webcam_info: Option<WebcamInfo>
}

impl SystemInfo {
    pub fn new() -> Self {
        Self {
            update_delay: 1000,
            watch_memory: false,
            watch_disk: false,
            watch_cpu: false,
            watch_cpu_clock_speed: false,
            watch_cpu_fan: false,
            watch_cpu_temperatures: false,
            watch_cpu_power: false,
            watch_gpu_clock_speed: false,
            watch_gpu_fan: false,
            watch_gpu_temperatures: false,
            watch_gpu_load: false,
            watch_process: false,
            watch_disk_speed: false,
            watch_network_speed: false,
            watch_net_ip: false,

            memory_info: EMPTY_STRING.to_string(),
            swap_info: EMPTY_STRING.to_string(),
            num_cpus: EMPTY_STRING.to_string(),
            cpu_brand: EMPTY_STRING.to_string(),
            cpu_usage_percpu: HashMap::new(),
            cpu_usage: EMPTY_STRING.to_string(),
            cpu_clock_speed: vec![],
            cpu_temperatures: vec![],
            cpu_temperature_total: 0.,
            cpu_cores_power: 0.,
            cpu_package_power: 0.,
            cpu_fans: vec![],
            gpu_clocks: vec![],
            gpu_fans: vec![],
            gpu_load: vec![],
            gpu_memory_load: vec![],
            gpu_load_total: vec![],
            gpu_memory_total: vec![],
            gpu_temperatures: vec![],
            gpu_cores_power: 0.,
            gpu_package_power: 0.,
            gpu_temperature_total: vec![],
            num_process: EMPTY_STRING.to_string(),
            disk_usage: HashMap::new(),
            system_name: EMPTY_STRING.to_string(),
            kernel_version: sysinfo::System::kernel_version().unwrap_or(String::from("N/A")),
            os_version: sysinfo::System::os_version().unwrap_or(String::from("N/A")),
            host_name: sysinfo::System::host_name().unwrap_or(String::from("N/A")),
            watch_weather: None,
            weather_info: None,
            cpu_freq_query_task: None,
            disk_speed_per_sec: (EMPTY_STRING.to_string(), EMPTY_STRING.to_string()),
            watch_disk_speed_task: None,
            watch_network_speed_task: None,
            network_speed_per_sec: (EMPTY_STRING.to_string(), EMPTY_STRING.to_string()),
            memory_percent: EMPTY_STRING.to_string(),
            swap_percent: EMPTY_STRING.to_string(),
            hardware_monitor_service: None,
            local_ip: EMPTY_STRING.to_string(),
            net_ip: None,
            webcam_frame: None,
            webcam_info: None,
            watch_webcam_task: None,
        }
    }
}

static SYSTEM_INFO: Lazy<Arc<RwLock<SystemInfo>>> = Lazy::new(|| {
    let ctx = Arc::new(RwLock::new(SystemInfo::new()));
    start_refresh_task(ctx.clone());
    ctx
});

fn try_write<'a, F: Fn(RwLockWriteGuard<'a, SystemInfo>)>(callback: F) {
    if let Ok(ctx) = SYSTEM_INFO.try_write() {
        callback(ctx);
    }
}

fn start_refresh_task(ctx: Arc<RwLock<SystemInfo>>) {
    std::thread::spawn(move || {
        let mut precord_core_system = None;

        match precord_core::System::new(
            precord_core::Features::PROCESS | precord_core::Features::CPU_FREQUENCY,
            [],
        ) {
            Ok(sys) => precord_core_system = Some(sys),
            Err(err) => error!("{:?}", err),
        };

        let mut sysinfo_system = sysinfo::System::new_all();
        let mut sysinfo_disks = sysinfo::Disks::new();

        let mut last_update_time = 0;
        let mut last_update_net_ip_time = 0;

        //(city, time)
        let last_weather_update_store: Arc<Mutex<(Option<City>, u128)>> =
            Arc::new(Mutex::new((None, 0)));

        loop {
            let current_time = current_timestamp();

            let update_delay = match ctx.read() {
                Err(_err) => 1000,
                Ok(ctx) => ctx.update_delay,
            };

            //相机根据帧率刷新
            let watch_webcam = match ctx.read() {
                Err(_err) => return,
                Ok(ctx) => ctx.webcam_info.is_some(),
            };

            //天气30分钟更新一次
            let watch_weather_data = match ctx.read() {
                Err(_err) => return,
                Ok(ctx) => ctx.watch_weather.clone(),
            };

            if let (Ok(mut last_weather_time), Some(city)) =
                (last_weather_update_store.lock(), watch_weather_data)
            {
                if let Some(last_city) = last_weather_time.0.as_ref() {
                    if last_city.code != city.code {
                        last_weather_time.1 = 0;
                    }
                }
                last_weather_time.0.replace(city.clone());
                if current_time - last_weather_time.1 > UPDATE_WEATHER_DELAY {
                    last_weather_time.1 = current_time;
                    std::thread::spawn(move || {
                        info!("开始更新天气 {:?}", city);
                        let weather = match query_weather(&city.code) {
                            Err(err) => {
                                error!("天气更新失败:{:?}", err);
                                return;
                            }
                            Ok(info) => info,
                        };
                        info!("天气已更新:{:?}", weather);
                        if let Ok(mut ctx) = SYSTEM_INFO.write() {
                            ctx.weather_info = Some(weather);
                        }
                    });
                }
            }

            //公网地址更新
            if current_time - last_update_net_ip_time > UPDATE_NET_IP_DELAY {
                let mut watch_net_ip = false;
                if let Ok(ctx) = SYSTEM_INFO.read() {
                    watch_net_ip = ctx.watch_net_ip;
                    drop(ctx);
                }

                if watch_net_ip {
                    last_update_net_ip_time = current_time;
                    std::thread::spawn(|| {
                        if let Ok(net_ip_info) = query_net_ip() {
                            if let Ok(mut ctx) = SYSTEM_INFO.write() {
                                ctx.net_ip = Some(net_ip_info);
                            }
                        }
                    });
                }
            }

            //系统信息1秒钟更新一次
            if current_time - last_update_time > update_delay {
                last_update_time = current_time;

                try_write(|mut ctx| {
                    ctx.system_name = sysinfo::System::name().unwrap_or(String::from("N/A"));
                    ctx.kernel_version =
                        sysinfo::System::kernel_version().unwrap_or(String::from("N/A"));
                    ctx.os_version = sysinfo::System::os_version().unwrap_or(String::from("N/A"));
                    ctx.host_name = sysinfo::System::host_name().unwrap_or(String::from("N/A"));
                    ctx.local_ip = match local_ip_address::local_ip() {
                        Ok(addr) => match addr {
                            std::net::IpAddr::V4(v4) => v4.to_string(),
                            std::net::IpAddr::V6(v6) => v6.to_string(),
                        },
                        Err(_) => "N/A".to_string(),
                    }
                });

                let mut watch_cpu = false;
                let mut watch_memory = false;
                let mut watch_disk = false;
                let mut watch_process = false;
                let mut watch_cpu_clock_speed = false;
                let mut watch_disk_speed = false;
                let mut watch_network_speed = false;

                #[cfg(target_os = "linux")]
                let mut watch_cpu_temperature = false;

                if let Ok(ctx) = SYSTEM_INFO.read() {
                    watch_cpu = ctx.watch_cpu;
                    watch_cpu_clock_speed = ctx.watch_cpu_clock_speed;
                    watch_memory = ctx.watch_memory;
                    watch_disk = ctx.watch_disk;
                    watch_process = ctx.watch_process;
                    watch_disk_speed = ctx.watch_disk_speed;
                    watch_network_speed = ctx.watch_network_speed;
                    drop(ctx);
                }

                if watch_cpu {
                    sysinfo_system.refresh_cpu();
                    let cpus = sysinfo_system.cpus();
                    let cpu_usage = sysinfo_system.global_cpu_info().cpu_usage();

                    try_write(|mut ctx| {
                        ctx.num_cpus = format!("{}", cpus.len());
                        ctx.cpu_brand = match cpus.get(0) {
                            Some(cpu) => cpu.brand().to_string().trim().to_string(),
                            None => EMPTY_STRING.to_string(),
                        };
                        for (cpu_idx, cpu) in cpus.iter().enumerate() {
                            ctx.cpu_usage_percpu
                                .insert(cpu_idx, format!("{:.1}%", cpu.cpu_usage()));
                        }
                        ctx.cpu_usage = format!("{:.1}%", cpu_usage);
                    });
                }
                if watch_memory {
                    sysinfo_system.refresh_memory();
                    try_write(|mut ctx| {
                        ctx.memory_info = format!(
                            "{}/{}GB",
                            bytes_to_gb(sysinfo_system.used_memory()),
                            bytes_to_gb(sysinfo_system.total_memory())
                        );
                        ctx.swap_info = format!(
                            "{}/{}GB",
                            bytes_to_gb(sysinfo_system.used_swap()),
                            bytes_to_gb(sysinfo_system.total_swap())
                        );
                        ctx.memory_percent = format!(
                            "{}%",
                            ((sysinfo_system.used_memory() as f64
                                / sysinfo_system.total_memory() as f64)
                                * 100.) as usize
                        );
                        ctx.swap_percent = format!(
                            "{}%",
                            ((sysinfo_system.used_swap() as f64
                                / sysinfo_system.total_swap() as f64)
                                * 100.) as usize
                        );
                    });
                }
                if watch_disk {
                    sysinfo_disks.refresh_list();
                    try_write(|mut ctx| {
                        for (disk_idx, disk) in sysinfo_disks.iter().enumerate() {
                            let path = disk.mount_point().to_str().unwrap_or("").replace("\\", "");
                            ctx.disk_usage.insert(
                                disk_idx,
                                format!(
                                    "({}) {}/{}GB",
                                    path,
                                    bytes_to_gb(disk.total_space() - disk.available_space()),
                                    bytes_to_gb(disk.total_space())
                                ),
                            );
                        }
                    });
                }

                if watch_disk_speed {
                    try_write(|mut ctx| {
                        if ctx.watch_disk_speed_task.is_none() {
                            ctx.watch_disk_speed_task = Some(start_disk_counter_thread());
                        }
                    });
                }

                #[cfg(any(feature = "nokhwa-webcam", feature = "v4l-webcam"))]
                if watch_webcam {
                    try_write(|mut ctx| {
                        if ctx.watch_webcam_task.is_none() {
                            #[cfg(any(feature = "nokhwa-webcam", all(not(windows),feature = "v4l-webcam")))]
                            {
                                ctx.watch_webcam_task = Some(start_webcam_capture_thread());
                            }
                        }
                    });
                }

                if watch_network_speed {
                    try_write(|mut ctx| {
                        if ctx.watch_network_speed_task.is_none() {
                            ctx.watch_network_speed_task = Some(start_network_counter_thread());
                        }
                    });
                }

                if watch_process {
                    sysinfo_system.refresh_processes();
                    try_write(|mut ctx| {
                        ctx.num_process = format!("{}", sysinfo_system.processes().keys().len());
                    });
                }

                if let Some(system) = precord_core_system.as_mut() {
                    if watch_cpu_clock_speed {
                        system.update(Instant::now());

                        if watch_cpu_clock_speed {
                            let mut freq_list = vec![];

                            if let Ok(v) = system.system_cpu_frequency() {
                                freq_list = v;
                            }

                            try_write(move |mut ctx| {
                                ctx.cpu_clock_speed = freq_list.clone();
                            });
                        }
                    }
                } else {
                    //precord_core_system 初始化失败，启动cpu主频获取线程
                    if watch_cpu_clock_speed {
                        #[cfg(windows)]
                        try_write(|mut ctx| {
                            if ctx.cpu_freq_query_task.is_none() {
                                ctx.cpu_freq_query_task = Some(start_get_cpu_freq_thread());
                            }
                        });
                    }
                }
            }

            std::thread::sleep(Duration::from_millis(10));
        }
    });
}

fn current_timestamp() -> u128 {
    let now = SystemTime::now();
    // 转换为UNIX纪元以来的纳秒数
    let since_the_epoch = now.duration_since(SystemTime::UNIX_EPOCH).unwrap();
    // 从纳秒转换为毫秒
    since_the_epoch.as_millis()
}

pub fn bytes_to_gb(bytes: u64) -> String {
    let kb = (bytes / 1024) as f64;
    let gb = kb / 1024. / 1024.;
    format!("{:.1}", gb)
}

fn try_read_ctx<'a>() -> Option<RwLockReadGuard<'a, SystemInfo>> {
    match SYSTEM_INFO.try_read() {
        Ok(sys) => Some(sys),
        Err(_) => None,
    }
}

// 设置刷新系统信息的延时时间,默认为1秒钟刷新一次
pub fn set_update_delay(update_delay: u128) -> Result<()> {
    let mut sys_info = SYSTEM_INFO.write().map_err(|err| anyhow!("{:?}", err))?;
    sys_info.update_delay = update_delay;
    Ok(())
}

pub fn watch_cpu(watch_cpu: bool) -> Result<()> {
    let mut sys_info = SYSTEM_INFO.write().map_err(|err| anyhow!("{:?}", err))?;
    sys_info.watch_cpu = watch_cpu;
    Ok(())
}

pub fn watch_cpu_clock_speed(watch_cpu_clock_speed: bool) -> Result<()> {
    let mut sys_info = SYSTEM_INFO.write().map_err(|err| anyhow!("{:?}", err))?;
    sys_info.watch_cpu_clock_speed = watch_cpu_clock_speed;
    info!("设置 watch_cpu_clock_speed:{watch_cpu_clock_speed}");
    Ok(())
}

pub fn watch_cpu_temperatures(val: bool) -> Result<()> {
    let mut sys_info = SYSTEM_INFO.write().map_err(|err| anyhow!("{:?}", err))?;
    sys_info.watch_cpu_temperatures = val;
    #[cfg(windows)]
    {
        if val {
            start_hardware_monitor_service(&mut *sys_info)?;
        }
    }
    Ok(())
}


pub fn watch_cpu_power(val: bool) -> Result<()> {
    let mut sys_info = SYSTEM_INFO.write().map_err(|err| anyhow!("{:?}", err))?;
    sys_info.watch_cpu_power = val;
    #[cfg(windows)]
    {
        if val {
            start_hardware_monitor_service(&mut *sys_info)?;
        }
    }
    Ok(())
}

pub fn watch_cpu_fan(val: bool) -> Result<()> {
    let mut sys_info = SYSTEM_INFO.write().map_err(|err| anyhow!("{:?}", err))?;
    sys_info.watch_cpu_fan = val;
    #[cfg(windows)]
    {
        if val {
            start_hardware_monitor_service(&mut *sys_info)?;
        }
    }
    Ok(())
}

pub fn watch_gpu_fan(val: bool) -> Result<()> {
    let mut sys_info = SYSTEM_INFO.write().map_err(|err| anyhow!("{:?}", err))?;
    sys_info.watch_gpu_fan = val;
    #[cfg(windows)]
    {
        if val {
            start_hardware_monitor_service(&mut *sys_info)?;
        }
    }
    Ok(())
}

pub fn watch_gpu_temperatures(val: bool) -> Result<()> {
    let mut sys_info = SYSTEM_INFO.write().map_err(|err| anyhow!("{:?}", err))?;
    sys_info.watch_gpu_temperatures = val;
    #[cfg(windows)]
    {
        if val {
            start_hardware_monitor_service(&mut *sys_info)?;
        }
    }
    Ok(())
}

pub fn watch_gpu_clock_speed(val: bool) -> Result<()> {
    let mut sys_info = SYSTEM_INFO.write().map_err(|err| anyhow!("{:?}", err))?;
    sys_info.watch_gpu_clock_speed = val;
    #[cfg(windows)]
    {
        if val {
            start_hardware_monitor_service(&mut *sys_info)?;
        }
    }
    Ok(())
}

pub fn watch_gpu_load(val: bool) -> Result<()> {
    let mut sys_info = SYSTEM_INFO.write().map_err(|err| anyhow!("{:?}", err))?;
    sys_info.watch_gpu_load = val;
    #[cfg(windows)]
    {
        if val {
            start_hardware_monitor_service(&mut *sys_info)?;
        }
    }
    Ok(())
}

pub fn watch_memory(watch_memory: bool) -> Result<()> {
    let mut sys_info = SYSTEM_INFO.write().map_err(|err| anyhow!("{:?}", err))?;
    sys_info.watch_memory = watch_memory;
    Ok(())
}

pub fn watch_disk(watch_disk: bool) -> Result<()> {
    let mut sys_info = SYSTEM_INFO.write().map_err(|err| anyhow!("{:?}", err))?;
    sys_info.watch_disk = watch_disk;
    Ok(())
}

pub fn watch_disk_speed(watch_disk_speed: bool) -> Result<()> {
    let mut sys_info = SYSTEM_INFO.write().map_err(|err| anyhow!("{:?}", err))?;
    sys_info.watch_disk_speed = watch_disk_speed;
    Ok(())
}

pub fn watch_network_speed(watch_network_speed: bool) -> Result<()> {
    let mut sys_info = SYSTEM_INFO.write().map_err(|err| anyhow!("{:?}", err))?;
    sys_info.watch_network_speed = watch_network_speed;
    Ok(())
}

pub fn watch_process(watch_process: bool) -> Result<()> {
    let mut sys_info = SYSTEM_INFO.write().map_err(|err| anyhow!("{:?}", err))?;
    sys_info.watch_process = watch_process;
    Ok(())
}

pub fn watch_weather(watch_weather: Option<City>) -> Result<()> {
    let mut sys_info = SYSTEM_INFO.write().map_err(|err| anyhow!("{:?}", err))?;
    sys_info.watch_weather = watch_weather;
    Ok(())
}

pub fn watch_net_ip(v: bool) -> Result<()> {
    let mut sys_info = SYSTEM_INFO.write().map_err(|err| anyhow!("{:?}", err))?;
    sys_info.watch_net_ip = v;
    Ok(())
}

pub fn watch_webcam(webcam_info: Option<WebcamInfo>) -> Result<()> {
    let mut sys_info = SYSTEM_INFO.write().map_err(|err| anyhow!("{:?}", err))?;
    sys_info.webcam_info = webcam_info;
    Ok(())
}

pub fn num_cpus() -> Option<String> {
    Some(try_read_ctx()?.num_cpus.clone())
}

pub fn cpu_brand() -> Option<String> {
    Some(try_read_ctx()?.cpu_brand.clone())
}

pub fn memory_info() -> Option<String> {
    Some(try_read_ctx()?.memory_info.clone())
}

pub fn memory_total() -> Option<String> {
    let info = try_read_ctx()?.memory_info.clone();
    let info = if info.contains("/") {
        info.split("/").last().unwrap().replace("GB", "G")
    } else {
        info
    };
    Some(info)
}

pub fn memory_percent() -> Option<String> {
    Some(try_read_ctx()?.memory_percent.clone())
}

pub fn swap_percent() -> Option<String> {
    Some(try_read_ctx()?.swap_percent.clone())
}

pub fn swap_info() -> Option<String> {
    Some(try_read_ctx()?.swap_info.clone())
}

pub fn cpu_usage_percpu(index: usize) -> Option<String> {
    try_read_ctx()?.cpu_usage_percpu.clone().remove(&index)
}

pub fn cpu_usage() -> Option<String> {
    Some(try_read_ctx()?.cpu_usage.clone())
}

pub fn webcam_frame() -> Option<RgbImage> {
    try_read_ctx()?.webcam_frame.clone()
}

pub fn cpu_clock_speed(index: Option<usize>) -> Option<String> {
    let cpu_clock_speed = try_read_ctx()?.cpu_clock_speed.clone();
    match index {
        Some(idx) => cpu_clock_speed
            .get(idx)
            .map(|v| format!("{:.2} GHz", v / 1000.)),
        None => cpu_clock_speed
            .into_iter()
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Less))
            .map(|v| format!("{:.2} GHz", v / 1000.)),
    }
}

pub fn cpu_temperature() -> Option<String> {
    let ctx = try_read_ctx()?;
    Some(format!("{:.1}°C", ctx.cpu_temperature_total))
}

pub fn cpu_cores_power() -> Option<String> {
    let ctx = try_read_ctx()?;
    Some(format!("{:.1}W", ctx.cpu_cores_power))
}

pub fn cpu_package_power() -> Option<String> {
    let ctx = try_read_ctx()?;
    Some(format!("{:.1}W", ctx.cpu_package_power))
}

pub fn cpu_fan() -> Option<String> {
    let ctx = try_read_ctx()?;
    if ctx.cpu_fans.len() == 0 {
        return None;
    }
    Some(format!("{}RPM", ctx.cpu_fans[0]))
}

pub fn gpu_load(index: usize) -> Option<String> {
    let ctx = try_read_ctx()?;
    let mut load_total = ctx.gpu_load_total.get(index).clone();

    if load_total.is_none(){
        return ctx.gpu_load.get(index).map(|loads|{
            let load = loads.get(0).unwrap_or(&0.);
            format!("{load}%")
        });
    }

    if let Some(t) = load_total {
        if *t == 0.0 && ctx.gpu_load.len() > 0 {
            if ctx.gpu_load[0].len() > 0 {
                load_total = Some(&ctx.gpu_load[0][0]);
            }
        }
    }
    load_total.map(|load| format!("{load}%"))
}

pub fn gpu_memory_load(index: usize) -> Option<String> {
    let ctx = try_read_ctx()?;

    return ctx.gpu_memory_load.get(index).map(|load|{
        format!("{:.1}%", load)
    });
}

pub fn gpu_memory_total_mb(index: usize) -> Option<String> {
    let ctx = try_read_ctx()?;
    return ctx.gpu_memory_total.get(index).map(|total|{
        format!("{total}")
    });
}

pub fn gpu_memory_total_gb(index: usize) -> Option<String> {
    let ctx = try_read_ctx()?;
    return ctx.gpu_memory_total.get(index).map(|total|{
        let gb = total / 1024.;
        format!("{:.1}", gb)
    });
}

pub fn gpu_clocks(index: usize) -> Option<String> {
    let gpu_clocks = try_read_ctx()?.gpu_clocks.clone();
    if gpu_clocks.len() == 0 {
        return None;
    }
    let idx = if gpu_clocks.len() > index { index } else { 0 };
    gpu_clocks[idx]
        .clone()
        .into_iter()
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Less))
        .map(|v| format!("{:.2} GHz", v / 1000.))
}

pub fn gpu_temperature(index: usize) -> Option<String> {
    let ctx = try_read_ctx()?;
    ctx.gpu_temperatures
        .get(index)
        .map(|t| format!("{:.1}°C", t.get(0).unwrap_or(&0.)))
}

pub fn gpu_cores_power() -> Option<String> {
    let ctx = try_read_ctx()?;
    Some(format!("{:.1}W", ctx.gpu_cores_power))
}

pub fn gpu_package_power() -> Option<String> {
    let ctx = try_read_ctx()?;
    Some(format!("{:.1}W", ctx.gpu_package_power))
}

pub fn gpu_fan(index: usize) -> Option<String> {
    let ctx = try_read_ctx()?;
    if ctx.gpu_fans.len() == 0 {
        return None;
    }
    if ctx.gpu_fans.len() <= index {
        return None;
    }
    if ctx.gpu_fans[index].len() == 0 {
        return None;
    }
    Some(format!("{}RPM", ctx.gpu_fans[index][0]))
}

pub fn num_process() -> Option<String> {
    Some(try_read_ctx()?.num_process.clone())
}

pub fn disk_usage(index: usize) -> Option<String> {
    try_read_ctx()?.disk_usage.clone().remove(&index)
}

pub fn disk_speed_per_sec() -> Option<(String, String)> {
    Some(try_read_ctx()?.disk_speed_per_sec.clone())
}

pub fn network_speed_per_sec() -> Option<(String, String)> {
    Some(try_read_ctx()?.network_speed_per_sec.clone())
}

pub fn system_name() -> Option<String> {
    Some(try_read_ctx()?.system_name.clone())
}

pub fn kernel_version() -> Option<String> {
    Some(try_read_ctx()?.kernel_version.clone())
}

pub fn os_version() -> Option<String> {
    Some(try_read_ctx()?.os_version.clone())
}

pub fn host_name() -> Option<String> {
    Some(try_read_ctx()?.host_name.clone())
}

pub fn date() -> String {
    Local::now().format("%Y/%m/%d").to_string()
}

pub fn time() -> String {
    Local::now().format("%H:%M:%S").to_string()
}

pub fn weather_info() -> Option<RealWeather> {
    try_read_ctx()?.weather_info.clone()
}

pub fn chinese_weekday() -> String {
    let weekday = Local::now().weekday();
    let week_days_chinese = [
        "星期日",
        "星期一",
        "星期二",
        "星期三",
        "星期四",
        "星期五",
        "星期六",
    ];
    week_days_chinese[weekday.num_days_from_sunday() as usize].to_string()
}

pub fn lunar_year() -> String {
    let now = Local::now();
    let d = SolorDate(now.year() as i32, now.month() as i32, now.day() as i32);
    // 时间12点
    let sz = d.sizhu(0.5);
    format!("农历{}年", sz.0)
}

pub fn lunar_date() -> String {
    let now = Local::now();
    const YM: [&str; 12] = [
        "正月", "二月", "三月", "四月", "五月", "六月", "七月", "八月", "九月", "十月", "冬月",
        "腊月",
    ];
    let x = SolorDate(now.year() as i32, now.month() as i32, now.day() as i32).to_lunar_date();
    // let year = x.0;
    let month = format!(
        "{}{}",
        if x.3 != 0 { "闰" } else { "" },
        YM[(x.1 as usize + 11) % 12]
    );
    let day = match x.2.to_chinese(
        ChineseVariant::Traditional,
        ChineseCase::Lower,
        ChineseCountMethod::TenThousand,
    ) {
        Ok(day) => day,
        Err(_err) => format!("{}", x.2),
    };
    format!("{month}{day}")
}

pub fn system_uptime() -> SystemUptime{
    let total_seconds = sysinfo::System::uptime();
    //减去天
    let days = total_seconds/(60*60*24);
    let total_seconds = total_seconds - days*(60*60*24);
    let duration = Duration::from_secs(total_seconds).human_duration();
    let duration = format!("{duration}");
    let arr:Vec<&str> = duration.split(":").collect();
    let mut uptime = SystemUptime::default();
    uptime.days = days as u32;
    if arr.len() > 2{
        uptime.hours = arr[0].parse().unwrap_or(0);
        uptime.minutes = arr[1].parse().unwrap_or(0);
        uptime.seconds = arr[2].parse().unwrap_or(0);
    }else if arr.len() > 1{
        uptime.minutes = arr[0].parse().unwrap_or(0);
        uptime.seconds = arr[1].parse().unwrap_or(0);
    }else if arr.len() > 0{
        uptime.seconds = arr[0].parse().unwrap_or(0);
    }
    uptime
}

pub fn net_ip_address() -> Option<String> {
    try_read_ctx()?.net_ip.as_ref().map(|i| i.query.clone())
}

pub fn net_ip_info() -> Option<String> {
    try_read_ctx()?
        .net_ip
        .as_ref()
        .map(|i| format!("{}{}{}", i.country, i.region_name, i.city))
}

pub fn local_ip_addresses() -> Option<String> {
    Some(try_read_ctx()?.local_ip.clone())
}

#[cfg(windows)]
fn start_get_cpu_freq_thread() -> std::thread::JoinHandle<()> {
    debug!("start_get_cpu_freq_thread...");
    std::thread::spawn(move || {
        //https://stackoverflow.com/questions/61802420/unable-to-get-current-cpu-frequency-in-powershell-or-python
        use std::mem::zeroed;
        use windows::core::w;
        use windows::Win32::System::Performance::{
            PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue, PDH_FMT_COUNTERVALUE,
            PDH_FMT_DOUBLE,
        };
        use windows::Win32::{
            Foundation::ERROR_SUCCESS,
            System::Performance::{PdhAddCounterW, PdhOpenQueryH},
        };

        let delay = Duration::from_millis(1000);
        loop {
            unsafe {
                let mut watch_cpu_clock_speed = false;
                if let Ok(ctx) = SYSTEM_INFO.read() {
                    watch_cpu_clock_speed = ctx.watch_cpu_clock_speed;
                    drop(ctx);
                }

                if !watch_cpu_clock_speed {
                    std::thread::sleep(delay);
                    continue;
                }

                //打开PDH
                let mut query = 0;
                let status = PdhOpenQueryH(0, 0, &mut query);
                if status != ERROR_SUCCESS.0 {
                    std::thread::sleep(delay);
                    continue;
                }

                let mut cpu_performance = 0;
                let mut cpu_basic_speed = 0;

                //添加CPU当前性能的计数器
                let status = PdhAddCounterW(
                    query,
                    w!("\\Processor Information(_Total)\\% Processor Performance"),
                    0,
                    &mut cpu_performance,
                );
                if status != ERROR_SUCCESS.0 {
                    std::thread::sleep(delay);
                    continue;
                }

                //添加CPU基准频率的计数器
                let status = PdhAddCounterW(
                    query,
                    w!("\\Processor Information(_Total)\\Processor Frequency"),
                    0,
                    &mut cpu_basic_speed,
                );
                if status != ERROR_SUCCESS.0 {
                    std::thread::sleep(delay);
                    continue;
                }

                //收集计数 因很多计数需要区间值 所以需要调用两次Query(间隔至少1s) 然后再获取计数值
                PdhCollectQueryData(query);
                std::thread::sleep(Duration::from_secs(1));
                PdhCollectQueryData(query);

                let mut pdh_value: PDH_FMT_COUNTERVALUE = zeroed();
                let mut dw_value = 0;

                let status = PdhGetFormattedCounterValue(
                    cpu_performance,
                    PDH_FMT_DOUBLE,
                    Some(&mut dw_value),
                    &mut pdh_value,
                );
                if status != ERROR_SUCCESS.0 {
                    std::thread::sleep(delay);
                    continue;
                }
                let cpu_performance = pdh_value.Anonymous.doubleValue / 100.0;

                let status = PdhGetFormattedCounterValue(
                    cpu_basic_speed,
                    PDH_FMT_DOUBLE,
                    Some(&mut dw_value),
                    &mut pdh_value,
                );
                if status != ERROR_SUCCESS.0 {
                    std::thread::sleep(delay);
                    continue;
                }
                let basic_speed = pdh_value.Anonymous.doubleValue;

                //关闭PDH
                PdhCloseQuery(query);

                try_write(move |mut ctx| {
                    ctx.cpu_clock_speed =
                        vec![(cpu_performance * basic_speed) as f32; num_cpus::get()];
                });
            }
        }
    })
}

// 网络需要单独按秒计数
pub fn start_network_counter_thread() -> std::thread::JoinHandle<()> {
    debug!("start_network_counter_thread...");
    std::thread::spawn(move || {
        let delay = Duration::from_millis(1000);
        let mut networks = Networks::new_with_refreshed_list();
        loop {
            let mut watch_network_speed = false;
            if let Ok(ctx) = SYSTEM_INFO.read() {
                watch_network_speed = ctx.watch_network_speed;
                drop(ctx);
            }

            if !watch_network_speed {
                std::thread::sleep(delay);
                continue;
            }

            networks.refresh();
            std::thread::sleep(delay);

            //只显示网速最大的网卡数据
            let mut received = 0;
            let mut transmitted = 0;
            let mut max = 0;
            for (_interface_name, data) in &networks {
                let tmp_max = data.received() + data.transmitted();
                if tmp_max > max {
                    max = tmp_max;
                    received = data.received();
                    transmitted = data.transmitted();
                }
            }

            let received_kb = received as f64 / 1024.;
            let transmitted_kb = transmitted as f64 / 1024.;
            let received_mb = received as f64 / 1024. / 1024.;
            let transmitted_mb = transmitted as f64 / 1024. / 1024.;

            let (received_str, transmitted_str) = (
                if received_mb >= 1. {
                    format!("{:.1}MB/s", received_mb)
                } else {
                    format!("{:.1}KB/s", received_kb)
                },
                if transmitted_mb >= 1. {
                    format!("{:.1}MB/s", transmitted_mb)
                } else {
                    format!("{:.1}KB/s", transmitted_kb)
                },
            );
            try_write(move |mut ctx| {
                ctx.network_speed_per_sec = (received_str.to_owned(), transmitted_str.to_owned());
            });
        }
    })
}

#[cfg(any(feature = "nokhwa-webcam", all(not(windows),feature = "v4l-webcam")))]
pub fn start_webcam_capture_thread() -> std::thread::JoinHandle<()> {
    debug!("start_webcam_capture_thread...");
    std::thread::spawn(move || {

        #[cfg(feature = "nokhwa-webcam")]
        let mut camera:Option<Camera> = None;
        #[cfg(all(not(windows),feature = "v4l-webcam", ))]
        let mut camera:Option<(v4l::Device, v4l::format::Format, v4l::prelude::MmapStream)> = None;
        
        let mut camera_index:i32 = -1;
        
        loop {
            let mut watch_webcam = None;
            if let Ok(ctx) = SYSTEM_INFO.read() {
                watch_webcam = ctx.webcam_info.clone();
                drop(ctx);
            }

            if watch_webcam.is_none() {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }else if let Some(webcam_info) = watch_webcam{
                if camera.is_none() || camera_index != webcam_info.index as i32{
                    camera_index = webcam_info.index as i32;
                    //相机需要重新打开
                    if camera.is_some(){
                        let cam = camera.take();
                        drop(cam);
                    }
                    info!("打开相机 camera_index={camera_index}");

                    #[cfg(feature = "nokhwa-webcam")]
                    {
                        let requested = RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate);
                        match Camera::new(CameraIndex::Index(camera_index as u32), requested){
                            Ok(cam) => camera = Some(cam),
                            Err(err) =>{
                                error!("相机打开失败:{err:?}");
                                std::thread::sleep(Duration::from_millis(3000));
                                continue;
                            }
                        };
                    }
                    #[cfg(all(not(windows),feature = "v4l-webcam", ))]
                    {
                        match open_v4l_webcam(camera_index){
                            Ok(cam) => camera = Some(cam),
                            Err(err) =>{
                                error!("相机打开失败:{err:?}");
                                std::thread::sleep(Duration::from_millis(3000));
                                continue;
                            }
                        };
                    }
                }

                if let Some(cam) = camera.as_mut(){
                    //开始拍照
                    let t = Instant::now();

                    let mut decoded_frame = None;
                    #[cfg(feature = "nokhwa-webcam")]
                    if let Ok(frame) = cam.frame(){
                        if let Ok(decoded) = frame.decode_image::<RgbFormat>(){
                            decoded_frame = Some(decoded);
                        }
                    }

                    #[cfg(all(not(windows),feature = "v4l-webcam", ))]
                    {
                        use v4l::io::traits::CaptureStream;
                        
                        let (dev, format, stream) = cam;
                        if let Ok((buf, meta)) = stream.next(){
                            decoded_frame = if format.fourcc == crate::yuv422::RGB3{
                                RgbImage::from_raw(format.width, format.height, buf.to_vec())
                            }else if format.fourcc == crate::yuv422::YUYV{
                                crate::yuv422::yuyv422_to_rgb(buf)
                                .map(|rgb| RgbImage::from_raw(format.width, format.height, rgb.to_vec()))
                                .unwrap_or(None)
                            }else if format.fourcc == crate::yuv422::MJPG{
                                image::load_from_memory_with_format(buf, image::ImageFormat::Jpeg)
                                .map(|img: image::DynamicImage| Some(img.to_rgb8()))
                                .unwrap_or(None)
                            }
                            else {
                                None
                            };
                        }
                    }

                    if let Some(decoded) = decoded_frame{
                        // info!("拍照大小:{}x{}", decoded.width(), decoded.height());
                        //缩放，最大不超过屏幕大小
                        let mut dst_width = decoded.width();
                        let mut dst_height = decoded.height();
                        // info!("图像缩放前大小:{dst_width}x{dst_height}");
                        if dst_width> webcam_info.width{
                            let scale = webcam_info.width as f32 / dst_width as f32;
                            dst_width = webcam_info.width;
                            dst_height = (scale*dst_height as f32) as u32;
                        }
                        if dst_height> webcam_info.height{
                            let scale = webcam_info.height as f32 / dst_height as f32;
                            dst_height = webcam_info.height;
                            dst_width = (scale*dst_width as f32) as u32;
                        }
                        // info!("图像缩放后大小:{dst_width}x{dst_height}");
                        let mut dst_image = Image::new(
                            dst_width,
                            dst_height,
                            fast_image_resize::PixelType::U8x3,
                        );

                        let mut src_image = Image::new(
                            decoded.width(),
                            decoded.height(),
                            fast_image_resize::PixelType::U8x3,
                        );
                        src_image.buffer_mut().copy_from_slice(&decoded);

                        // Create Resizer instance and resize source image
                        // into buffer of destination image
                        let mut resizer = Resizer::new();
                        let r = resizer.resize(&src_image, &mut dst_image, None);
                        if r.is_err(){
                            std::thread::sleep(Duration::from_millis(1000));
                            continue;
                        }

                        //写入缓存
                        try_write(move |mut ctx| {
                            if let Some(img) = RgbImage::from_raw(dst_image.width(), dst_image.height(), dst_image.buffer().to_vec()){
                                ctx.webcam_frame = Some(img);
                            }
                        });
                    }

                    //延迟，减去可能花费的拍照时间
                    let dur = t.elapsed().as_millis() as u64;
                    let delay = 1000/webcam_info.fps as u64;
                    if dur >= delay{
                        std::thread::sleep(Duration::from_millis(1));
                    }else{
                        std::thread::sleep(Duration::from_millis(delay - dur));
                    }
                }
            }
        }
    })
}

#[cfg(windows)]
pub fn start_disk_counter_thread() -> std::thread::JoinHandle<()> {
    debug!("start_disk_counter_thread...");
    std::thread::spawn(move || {
        //https://stackoverflow.com/questions/61802420/unable-to-get-current-cpu-frequency-in-powershell-or-python
        use std::mem::zeroed;
        use windows::core::w;
        use windows::Win32::System::Performance::{
            PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue, PDH_FMT_COUNTERVALUE,
            PDH_FMT_DOUBLE,
        };
        use windows::Win32::{
            Foundation::ERROR_SUCCESS,
            System::Performance::{PdhAddCounterW, PdhOpenQueryH},
        };

        let delay = Duration::from_millis(1000);
        loop {
            unsafe {
                let mut watch_disk_speed = false;
                if let Ok(ctx) = SYSTEM_INFO.read() {
                    watch_disk_speed = ctx.watch_disk_speed;
                    drop(ctx);
                }

                if !watch_disk_speed {
                    std::thread::sleep(delay);
                    continue;
                }

                //打开PDH
                let mut query = 0;
                let status = PdhOpenQueryH(0, 0, &mut query);
                if status != ERROR_SUCCESS.0 {
                    std::thread::sleep(delay);
                    continue;
                }

                let mut read_bytes_per_sec_counter = 0;
                let mut write_bytes_per_sec_counter = 0;

                let status = PdhAddCounterW(
                    query,
                    w!("\\PhysicalDisk(_Total)\\Disk Read Bytes/sec"),
                    0,
                    &mut read_bytes_per_sec_counter,
                );
                if status != ERROR_SUCCESS.0 {
                    std::thread::sleep(delay);
                    continue;
                }

                let status = PdhAddCounterW(
                    query,
                    w!("\\PhysicalDisk(_Total)\\Disk Write Bytes/sec"),
                    0,
                    &mut write_bytes_per_sec_counter,
                );
                if status != ERROR_SUCCESS.0 {
                    std::thread::sleep(delay);
                    continue;
                }

                PdhCollectQueryData(query);
                std::thread::sleep(Duration::from_secs(1));
                PdhCollectQueryData(query);

                let mut pdh_value: PDH_FMT_COUNTERVALUE = zeroed();
                let mut dw_value = 0;

                let status = PdhGetFormattedCounterValue(
                    read_bytes_per_sec_counter,
                    PDH_FMT_DOUBLE,
                    Some(&mut dw_value),
                    &mut pdh_value,
                );
                if status != ERROR_SUCCESS.0 {
                    std::thread::sleep(delay);
                    continue;
                }
                let read_bytes_per_sec = pdh_value.Anonymous.doubleValue;

                let status = PdhGetFormattedCounterValue(
                    write_bytes_per_sec_counter,
                    PDH_FMT_DOUBLE,
                    Some(&mut dw_value),
                    &mut pdh_value,
                );
                if status != ERROR_SUCCESS.0 {
                    std::thread::sleep(delay);
                    continue;
                }
                let write_bytes_per_sec = pdh_value.Anonymous.doubleValue;

                //关闭PDH
                PdhCloseQuery(query);

                let read_str = format!("{:.1} MB/s", read_bytes_per_sec / 1024. / 1024.);
                let write_str = format!("{:.1} MB/s", write_bytes_per_sec / 1024. / 1024.);
                try_write(move |mut ctx| {
                    ctx.disk_speed_per_sec = (read_str.to_owned(), write_str.to_owned());
                });
            }
        }
    })
}

#[cfg(not(windows))]
pub fn start_disk_counter_thread() -> std::thread::JoinHandle<()> {
    use psutil::disk;

    debug!("start_disk_counter_thread...");

    std::thread::spawn(move || {
        let delay = Duration::from_millis(1000);
        let mut disk_io_counters_collector = disk::DiskIoCountersCollector::default();
        let mut prev_disk_io_counters = match disk_io_counters_collector.disk_io_counters() {
            Err(err) => {
                error!("disk_io_counters:{:?}", err);
                return;
            }
            Ok(v) => v,
        };
        loop {
            let mut watch_disk_speed = false;
            if let Ok(ctx) = SYSTEM_INFO.read() {
                watch_disk_speed = ctx.watch_disk_speed;
                drop(ctx);
            }

            if !watch_disk_speed {
                std::thread::sleep(delay);
                continue;
            }

            std::thread::sleep(delay);

            let current_disk_io_counters = disk_io_counters_collector.disk_io_counters().unwrap();

            let counter = current_disk_io_counters.clone() - prev_disk_io_counters;

            prev_disk_io_counters = current_disk_io_counters;

            let read_str = format!("{:.1} MB/s", counter.read_bytes() as f64 / 1024. / 1024.);
            let write_str = format!("{:.1} MB/s", counter.write_bytes() as f64 / 1024. / 1024.);
            try_write(move |mut ctx| {
                ctx.disk_speed_per_sec = (read_str.to_owned(), write_str.to_owned());
            });
        }
    })
}

#[cfg(windows)]
pub static HTTP_PORT: Lazy<u16> = Lazy::new(|| {
    use tiny_http::{Response, Server};
    let server = Server::http("0.0.0.0:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    std::thread::spawn(move || {
        for mut request in server.incoming_requests() {
            info!(
                "received request! method: {:?}, url: {:?}, headers: {:?}",
                request.method(),
                request.url(),
                request.headers()
            );

            let url = request.url();

            if url.contains("isOpen") {
                let is_open = if let Ok(ctx) = SYSTEM_INFO.read() {
                    ctx.watch_cpu_fan
                        || ctx.watch_cpu_temperatures
                        || ctx.watch_cpu_power
                        || ctx.watch_gpu_clock_speed
                        || ctx.watch_gpu_fan
                        || ctx.watch_gpu_load
                        || ctx.watch_gpu_temperatures
                } else {
                    false
                };
                let _ = request.respond(Response::from_string(if is_open {
                    "true"
                } else {
                    "false"
                }));
            } else if url.contains("upload") {
                let reader = request.as_reader();
                let mut buf = vec![];
                let _ = reader.read_to_end(&mut buf);
                if buf.len() > 0 {
                    if let Ok(json) = String::from_utf8(buf.to_vec()) {
                        info!("接收到:{json}");
                        if let Ok(info) = serde_json::from_str::<HardwareData>(&json) {
                            if let Ok(mut ctx) = SYSTEM_INFO.write() {
                                if info.cpu_infos.len() > 0 {
                                    ctx.cpu_temperatures = info.cpu_infos[0].temperatures.clone();
                                    ctx.cpu_fans = info.cpu_infos[0].fans.clone();
                                    ctx.cpu_temperature_total = info.cpu_infos[0].total_temperature;
                                    ctx.cpu_cores_power = info.cpu_infos[0].cores_power;
                                    ctx.cpu_package_power = info.cpu_infos[0].package_power;
                                }
                                ctx.gpu_clocks.clear();
                                ctx.gpu_fans.clear();
                                ctx.gpu_load.clear();
                                ctx.gpu_temperatures.clear();
                                ctx.gpu_temperature_total.clear();
                                ctx.gpu_load_total.clear();
                                ctx.gpu_memory_load.clear();
                                ctx.gpu_memory_total.clear();
                                for gpu_info in info.gpu_infos {
                                    ctx.gpu_clocks.push(gpu_info.clocks.clone());
                                    ctx.gpu_temperatures.push(gpu_info.temperatures.clone());
                                    ctx.gpu_fans.push(gpu_info.fans.clone());
                                    ctx.gpu_load.push(gpu_info.loads.clone());
                                    ctx.gpu_temperature_total.push(gpu_info.total_temperature);
                                    ctx.gpu_load_total.push(gpu_info.total_load);
                                    ctx.gpu_cores_power = gpu_info.cores_power;
                                    ctx.gpu_package_power = gpu_info.package_power;
                                    ctx.gpu_memory_load.push(gpu_info.memory_load);
                                    ctx.gpu_memory_total.push(gpu_info.memory_total);
                                }
                            }
                        }
                    }
                }
                let _ = request.respond(Response::from_string("OK"));
            }
        }
    });
    port
});

#[cfg(windows)]
fn start_hardware_monitor_service(ctx: &mut SystemInfo) -> Result<()> {
    //以管理员身份启动
    match crate::is_run_as_admin() {
        Ok(true) => (),
        _ => {
            info!("提示保存数据");
            std::thread::spawn(|| {
                let ret = MessageDialog::new()
                        .set_description("监测CPU、GPU温度，需要以管理员权限重新启动程序。\n请确认已保存当前画布文件！")
                        .set_buttons(rfd::MessageButtons::OkCancel)
                        .show();
                if let MessageDialogResult::Ok = ret {
                    info!("请求管理员身份启动...");
                    if let Err(err) = crate::run_as_admin(None) {
                        MessageDialog::new()
                            .set_description(format!("{:?}", err))
                            .set_buttons(rfd::MessageButtons::Ok)
                            .show();
                    } else {
                        std::process::exit(0);
                    }
                }
            });
            return Ok(());
        }
    };

    use std::process::Command;

    use rfd::{MessageDialog, MessageDialogResult};

    //如果进程已经启动，不再创建
    if let Some(child) = ctx.hardware_monitor_service.as_mut() {
        if let Ok(None) = child.try_wait() {
            return Ok(());
        }
    }

    ctx.hardware_monitor_service = None;

    //判断exe是否存在
    let exe_path = "./OpenHardwareMonitorService.exe";

    if let Err(_) = std::fs::metadata(exe_path) {
        //exe文件不存在，重写创建
        std::fs::write(exe_path, OHMS_EXE_FILE)?;
        info!("exe文件创建成功.");
    }
    info!("启动exe...");

    let child = Command::new(exe_path)
        .arg(format!("{}", *HTTP_PORT))
        .spawn()?;
    let pid = child.id();
    info!("{}进程启动:{}", exe_path, pid);

    ctx.hardware_monitor_service = Some(child);

    Ok(())
}

pub fn clean() {
    #[cfg(windows)]
    {
        let exe_path = "OpenHardwareMonitorService.exe";
        //结束进程
        if let Ok(mut ctx) = SYSTEM_INFO.write().map_err(|err| anyhow!("{:?}", err)) {
            if let Some(mut process) = ctx.hardware_monitor_service.take() {
                let ret = process.kill();
                info!("进程已结束:{:?}", ret);
                let ret = process.wait();
                info!("wait:{:?}", ret);
            }
        }

        match std::fs::metadata(exe_path) {
            // 如果成功，文件存在
            Ok(_) => {
                let ret = std::fs::remove_file(exe_path);
                info!("删除exe:{:?}", ret);
            }
            _ => (),
        }
    }
}

pub fn query_net_ip() -> Result<NetIpInfo> {
    let json = reqwest::blocking::get("http://ip-api.com/json/?lang=zh-CN")?.text()?;
    // info!("天气:{json}");
    let resp = serde_json::from_str::<NetIpInfo>(&json)?;
    Ok(resp)
}

#[cfg(all(not(windows),feature = "v4l-webcam", ))]
pub fn open_v4l_webcam<'a>(index: i32) -> Result<(v4l::Device, v4l::format::Format, v4l::prelude::MmapStream<'a>)>{
    // Allocate 4 buffers by default
    let buffer_count = 4;
    let dev = v4l::Device::with_path(format!("/dev/video{index}"))?;
    let formats = v4l::video::Capture::enum_formats(&dev).unwrap_or(vec![]);
    for desc in formats{
        //首选MJPG
        if desc.fourcc == crate::yuv422::MJPG{
            let _ = v4l::video::Capture::set_format(&dev, &v4l::Format::new(320, 240, crate::yuv422::MJPG));
            break;
        }else if desc.fourcc == crate::yuv422::RGB3{
            let _ = v4l::video::Capture::set_format(&dev, &v4l::Format::new(320, 240, crate::yuv422::RGB3));
            break;
        }
    }

    let format = v4l::video::Capture::format(&dev)?;
    let params = v4l::video::Capture::params(&dev)?;
    info!("当前选中的格式:\n{}", format);
    info!("当前选中的参数:\n{}", params);

    // Setup a buffer stream and grab a frame, then print its data
    let mut stream = v4l::prelude::MmapStream::with_buffers(&dev, v4l::buffer::Type::VideoCapture, buffer_count)?;

    // warmup
    v4l::io::traits::CaptureStream::next(&mut stream)?;
    Ok((dev, format, stream))
}