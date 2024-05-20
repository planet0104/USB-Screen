use anyhow::Result;
use image::RgbaImage;
use log::info;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

pub const CITIES: Lazy<Vec<City>> =
    Lazy::new(|| serde_json::from_str(include_str!("../cities.json")).unwrap());

pub const ICONS: Lazy<Vec<RgbaImage>> = Lazy::new(|| {
    vec![
        image::load_from_memory(include_bytes!("../images/0.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/1.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/2.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/3.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/4.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/5.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/6.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/7.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/8.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/9.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/10.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/11.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/12.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/13.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/14.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/15.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/16.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/17.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/18.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/19.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/20.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/21.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/22.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/23.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/24.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/25.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/26.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/27.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/28.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/29.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/30.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/31.png"))
            .unwrap()
            .to_rgba8(),
        image::load_from_memory(include_bytes!("../images/32.png"))
            .unwrap()
            .to_rgba8(),
    ]
});

#[derive(Debug, Serialize, Deserialize)]
pub struct Province {
    code: String,
    name: String,
    url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct City {
    pub code: String,
    pub province: String,
    pub city: String,
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WeatherResp {
    msg: String,
    code: i32,
    data: WeatherData,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WeatherData {
    real: RealWeather,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealWeather {
    pub station: City,
    pub publish_time: String,
    pub weather: Weather,
    pub wind: Wind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Weather {
    pub temperature: f32,
    #[serde(rename = "temperatureDiff")]
    pub temperature_diff: f32,
    pub airpressure: f32,
    pub humidity: f32,
    pub rain: f32,
    pub rcomfort: f32,
    pub icomfort: f32,
    pub info: String,
    pub img: String,
    pub feelst: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wind {
    pub direct: String,
    pub degree: f32,
    pub power: String,
    pub speed: f32,
}

#[allow(unused)]
pub fn query_province() -> Result<Vec<Province>> {
    let json = reqwest::blocking::get("http://www.nmc.cn/rest/province")?.text()?;
    info!("获取省份:");
    info!("{json}");
    Ok(serde_json::from_str(&json)?)
}

#[allow(unused)]
pub fn query_city() -> Result<Vec<City>> {
    let mut cities = vec![];
    for p in query_province()? {
        let json = reqwest::blocking::get(format!("http://www.nmc.cn/rest/province/{}", p.code))?
            .text()?;
        info!("获取城市({}):{json}", p.name);
        for city in serde_json::from_str::<Vec<City>>(&json)? {
            cities.push(city);
        }
    }
    Ok(cities)
}

pub fn query_weather(station_id: &str) -> Result<RealWeather> {
    let json = reqwest::blocking::get(format!(
        "http://www.nmc.cn/rest/weather?stationid={station_id}"
    ))?
    .text()?;
    // info!("天气:{json}");
    let resp = serde_json::from_str::<WeatherResp>(&json)?;
    Ok(resp.data.real)
}

#[test]
fn download_city() -> Result<()> {
    use std::io::Write;
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .try_init()?;
    let cities = query_city()?;
    let json = serde_json::to_string(&cities)?;
    let mut file = std::fs::File::create("cities.json")?;
    file.write_all(json.as_bytes())?;
    Ok(())
}

#[test]
fn test_weather() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .try_init()?;
    info!("城市数量:{}", CITIES.len());
    for city in CITIES.iter() {
        if city.city.contains("松江") {
            let weather = query_weather(&city.code)?;

            let weather_info = weather.weather.info;
            let temperature = weather.weather.temperature;
            let winddirection = weather.wind.direct;
            let windpower = weather.wind.speed;

            let info = format!("{weather_info} {temperature}℃  {winddirection}{windpower}级");

            info!("{info}");
            break;
        }
    }
    Ok(())
}
