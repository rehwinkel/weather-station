use std::{
    path::Path,
    sync::Arc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use clap::Parser;
use rouille::Response;
use serde::Serialize;
use sqlite::State;
use tracing::info;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(long, default_value_t = 17, help = "RaspberryPi GPIO pin number")]
    gpio_pin: u8,
    #[arg(long, default_value = "./weather.sqlite", help = "SQLite DB file")]
    db_file: String,
    #[arg(long, default_value_t = 8080, help = "Web server port")]
    port: u16,
    #[arg(long, default_value = "127.0.0.1", help = "Web server host")]
    host: String,
    #[arg(
        long,
        default_value_t = 60,
        help = "Sensor reading interval in seconds"
    )]
    interval: u64,
}

const INDEX_HTML: &'static str = include_str!("index.html");
const FAVICON: &'static [u8] = include_bytes!("favicon.ico");

#[derive(Debug, Serialize)]
struct WeatherData {
    time: u64,
    temperature_celsius: f32,
    humidity_percent: f32,
}

#[derive(Debug, Serialize)]
struct ResponseData {
    data: Vec<WeatherData>,
    stat: WeatherAveragesMedians,
}

#[derive(Debug, Serialize)]
struct WeatherAveragesMedians {
    average_temperature_celsius: f32,
    average_humidity_percent: f32,
    median_temperature_celsius: f32,
    median_humidity_percent: f32,
}

fn find_average_and_median(data: &[WeatherData]) -> WeatherAveragesMedians {
    let (mut sorted_temps, mut sorted_humidities): (Vec<f32>, Vec<f32>) = data
        .iter()
        .map(|d| (d.temperature_celsius, d.humidity_percent))
        .unzip();
    sorted_temps.sort_by(|a, b| a.partial_cmp(b).unwrap());
    sorted_humidities.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_temperature_celsius = if sorted_temps.len() > 0 {
        sorted_temps[sorted_temps.len() / 2]
    } else {
        f32::NAN
    };
    let median_humidity_percent = if sorted_humidities.len() > 0 {
        sorted_humidities[sorted_humidities.len() / 2]
    } else {
        f32::NAN
    };
    let average_temperature_celsius = if sorted_temps.len() > 0 {
        sorted_temps.iter().sum::<f32>() / sorted_temps.len() as f32
    } else {
        f32::NAN
    };
    let average_humidity_percent = if sorted_humidities.len() > 0 {
        sorted_humidities.iter().sum::<f32>() / sorted_humidities.len() as f32
    } else {
        f32::NAN
    };
    WeatherAveragesMedians {
        average_temperature_celsius,
        average_humidity_percent,
        median_temperature_celsius,
        median_humidity_percent,
    }
}

fn main() {
    let args = Args::parse();

    tracing_subscriber::fmt::init();

    let db_file_path = Path::new(args.db_file.as_str());
    let db = Arc::new(setup_database(db_file_path));

    let sensor_db = db.clone();
    thread::spawn(move || loop {
        let reading_time = read_sensor_and_store(&sensor_db, args.gpio_pin);
        let time_to_wait = std::time::Duration::from_secs(args.interval) - reading_time;
        std::thread::sleep(time_to_wait);
    });

    rouille::start_server_with_pool(
        (args.host, args.port),
        Some(4),
        move |request| match request.url().as_str() {
            "/" => Response::html(INDEX_HTML),
            "/data" => {
                let mut stmt = db.prepare("SELECT unix_time, temperature_celsius_q, humidity_percent_q FROM weather_readings WHERE unix_time BETWEEN ? AND ? ORDER BY unix_time;").unwrap();
                let now = SystemTime::now();
                let start = now - Duration::from_secs(3600);
                stmt.bind((
                    1,
                    start.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64,
                ))
                .unwrap();
                stmt.bind((2, now.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64))
                    .unwrap();
                let mut data = Vec::new();
                while let Ok(State::Row) = stmt.next() {
                    let time = stmt.read::<i64, _>("unix_time").unwrap();
                    let temperature_celsius_q =
                        stmt.read::<i64, _>("temperature_celsius_q").unwrap();
                    let humidity_percent_q = stmt.read::<i64, _>("humidity_percent_q").unwrap();
                    data.push(WeatherData {
                        time: time as u64,
                        temperature_celsius: temperature_celsius_q as f32 / 4.0,
                        humidity_percent: humidity_percent_q as f32 / 4.0,
                    });
                }
                let stat = find_average_and_median(&data);
                let response_data = ResponseData { data, stat };
                Response::json(&response_data)
            }
            "/favicon.ico" => Response::from_data("image/x-icon", FAVICON),
            _ => Response::empty_404(),
        },
    );
}

fn read_sensor_and_store(db: &sqlite::Connection, gpio_pin: u8) -> std::time::Duration {
    let now = std::time::Instant::now();
    let reading = loop {
        let reading_res = dht22_pi::read(gpio_pin);
        if let Ok(reading) = reading_res {
            break reading;
        }
    };
    info!("Reading: {:?}", reading);
    let temp = (reading.temperature * 4.0).round() as i64;
    let hum = (reading.humidity * 4.0).round() as i64;
    let mut stmt = db.prepare("INSERT INTO weather_readings (unix_time, temperature_celsius_q, humidity_percent_q) VALUES (?, ?, ?);").unwrap();
    stmt.bind((
        1,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64,
    ))
    .unwrap();
    stmt.bind((2, temp)).unwrap();
    stmt.bind((3, hum)).unwrap();
    stmt.next().unwrap();
    let reading_time = now.elapsed();
    reading_time
}

fn connect_database(db_file_path: &Path) -> sqlite::ConnectionThreadSafe {
    info!(
        "Connecting to SQLite DB file '{}'...",
        db_file_path.display()
    );
    let con = sqlite::Connection::open_thread_safe(db_file_path).unwrap();
    con
}

fn setup_database(db_file_path: &Path) -> sqlite::ConnectionThreadSafe {
    let con = connect_database(db_file_path);
    con.execute("CREATE TABLE IF NOT EXISTS weather_readings (unix_time INTEGER PRIMARY KEY, temperature_celsius_q INTEGER, humidity_percent_q INTEGER)").unwrap();
    con
}
