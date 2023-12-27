use tracing::info;

fn main() {
    tracing_subscriber::fmt::init();
    info!("Connecting to SQLite DB...");
    let con = sqlite::open("./weather.sqlite").unwrap();
    con.execute("CREATE TABLE IF NOT EXISTS weather_readings (time DATETIME DEFAULT CURRENT_TIMESTAMP PRIMARY KEY, temperature_celsius_q INTEGER, humidity_percent_q INTEGER)").unwrap();
    loop {
        let now = std::time::Instant::now();
        let reading = loop {
            let reading_res = dht22_pi::read(17);
            if let Ok(reading) = reading_res {
                break reading;
            }
        };
        info!("Reading: {:?}", reading);
        let temp = (reading.temperature * 4.0).round() as i64;
        let hum = (reading.humidity * 4.0).round() as i64;
        let mut stmt = con.prepare("INSERT INTO weather_readings (temperature_celsius_q, humidity_percent_q) VALUES (?, ?);").unwrap();
        stmt.bind((1, temp)).unwrap();
        stmt.bind((2, hum)).unwrap();
        stmt.next().unwrap();
        let reading_time = now.elapsed();
        let time_to_wait = std::time::Duration::from_secs(60) - reading_time;
        std::thread::sleep(time_to_wait);
    }
}
