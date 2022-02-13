use color_eyre::{eyre::WrapErr, Report, Result};
use json::JsonValue;
use rumqttc::{AsyncClient, QoS};
use std::collections::{BTreeMap, HashMap};
use std::convert::TryFrom;
use std::fmt::{self, Debug, Display, Formatter, Write};
use std::time::Instant;
use tokio::task::spawn;

#[derive(Default)]
pub struct DeviceStates {
    pub devices: HashMap<Device, DeviceState>,
    pub mi_temp_devices: BTreeMap<BDAddr, MiTempState>,
}

impl DeviceStates {
    pub fn devices(&self) -> impl Iterator<Item = (&Device, &DeviceState)> {
        self.devices.iter()
    }

    pub fn update(&mut self, device: Device, json: JsonValue) {
        let device = self.devices.entry(device).or_default();

        for (key, value) in json.entries() {
            if let Some(addr) = key.strip_prefix("MJ_HT_V1") {
                let addr = addr.trim_start_matches('-');
                match BDAddr::from_mi_temp_mac_part(addr) {
                    Ok(addr) => {
                        let state = self.mi_temp_devices.entry(addr).or_default();
                        state.update(value);
                    }
                    Err(e) => eprintln!("Failed to parse mitemp mac: {:#}", e),
                }
            }
        }

        device.update(json);
    }

    pub fn mi_temp(&self) -> impl Iterator<Item = (&BDAddr, &MiTempState)> {
        self.mi_temp_devices.iter()
    }

    pub fn retain(&mut self, cleanup_time: Instant, ping_time: Instant, client: &AsyncClient) {
        self.devices.retain(|device, state| {
            if state.last_seen < cleanup_time {
                println!("{} hasn't been seen for 15m, removing", device.hostname);
                false
            } else if state.last_seen < ping_time || state.name.is_empty() {
                println!(
                    "{} hasn't been seen for 10m or has no name set, pinging",
                    device.hostname
                );
                let send_client = client.clone();
                let topic = device.get_topic("cmnd", "DeviceName");
                spawn(async move {
                    if let Err(e) = send_client.publish(topic, QoS::AtMostOnce, false, "").await {
                        eprintln!("Failed to ping device: {:#}", e);
                    }
                });
                true
            } else {
                true
            }
        });

        self.mi_temp_devices.retain(|device, state| {
            if state.last_seen < cleanup_time {
                println!("{} hasn't been seen for 15m, removing", device);
                false
            } else {
                true
            }
        });
    }
}

#[derive(Debug, Eq, PartialEq, Clone, Hash)]
pub struct Device {
    pub hostname: String,
}

impl Device {
    pub fn get_topic(&self, prefix: &str, command: &str) -> String {
        format!("{}/{}/{}", prefix, self.hostname, command)
    }
}

#[derive(Debug)]
pub struct DeviceState {
    pub state: Option<bool>,
    pub name: String,
    pub power_watts: Option<f32>,
    pub power_yesterday: Option<f32>,
    pub power_today: Option<f32>,
    pub power_total: Option<f32>,
    pub power_total_low: Option<f32>,
    pub power_total_high: Option<f32>,
    pub gas_total: Option<f32>,
    pub co2: Option<f32>,
    pub pms_state: Option<PMSState>,
    pub last_seen: Instant,
    pub firmware: String,
    pub version: f32,
}

impl Default for DeviceState {
    fn default() -> Self {
        DeviceState {
            state: Default::default(),
            name: Default::default(),
            power_watts: Default::default(),
            power_yesterday: Default::default(),
            power_today: Default::default(),
            power_total: Default::default(),
            power_total_low: Default::default(),
            power_total_high: Default::default(),
            gas_total: Default::default(),
            co2: Default::default(),
            pms_state: Default::default(),
            last_seen: Instant::now(),
            firmware: Default::default(),
            version: 0.0,
        }
    }
}

impl DeviceState {
    pub fn update(&mut self, json: JsonValue) {
        self.last_seen = Instant::now();

        if json["DeviceName"].is_string() && !json["DeviceName"].is_empty() {
            self.name = json["DeviceName"].to_string();
        }
        if json["POWER"].is_string() && !json["POWER"].is_empty() {
            self.state = Some(json["POWER"] == "ON");
        }
        if let Some(power) = json["ENERGY"]["Power"]
            .as_number()
            .and_then(|num| f32::try_from(num).ok())
        {
            self.power_watts = Some(power);
        }
        if let Some(yesterday) = json["ENERGY"]["Yesterday"]
            .as_number()
            .and_then(|num| f32::try_from(num).ok())
        {
            self.power_yesterday = Some(yesterday);
        }
        if let Some(today) = json["ENERGY"]["Today"]
            .as_number()
            .and_then(|num| f32::try_from(num).ok())
        {
            self.power_today = Some(today);
        }
        if let Some(co2) = json["MHZ19B"]["CarbonDioxide"]
            .as_number()
            .and_then(|num| f32::try_from(num).ok())
        {
            if co2 > 1.0 {
                self.co2 = Some(co2);
            }
        }
        if let Some(power) = json["OBIS"]["Power"]
            .as_number()
            .and_then(|num| f32::try_from(num).ok())
        {
            self.power_watts = Some(power);
        }
        if let Some(total) = json["OBIS"]["Total"]
            .as_number()
            .and_then(|num| f32::try_from(num).ok())
        {
            self.power_total = Some(total);
        }
        if let Some(total) = json["OBIS"]["Total_high"]
            .as_number()
            .and_then(|num| f32::try_from(num).ok())
        {
            self.power_total_high = Some(total);
        }
        if let Some(total) = json["OBIS"]["Total_low"]
            .as_number()
            .and_then(|num| f32::try_from(num).ok())
        {
            self.power_total_low = Some(total);
        }
        if let Some(gas) = json["OBIS"]["Gas_total"]
            .as_number()
            .and_then(|num| f32::try_from(num).ok())
        {
            self.gas_total = Some(gas);
        }

        if let Some(version) = json["StatusFWR"]["Version"].as_str() {
            self.firmware = version.into();
            if let Some(version) = version
                .rfind(".")
                .map(|index| &version[0..index])
                .and_then(|s| s.parse().ok())
            {
                self.version = version
            }
        }

        if json["PMS5003"].is_object() {
            let pms = self.pms_state.get_or_insert(PMSState::default());
            pms.update(&json["PMS5003"]);
        }
    }
}

#[derive(Debug)]
pub struct MiTempState {
    temperature: f32,
    humidity: f32,
    dew_point: f32,
    battery: u8,
    pub last_seen: Instant,
}

impl Default for MiTempState {
    fn default() -> Self {
        MiTempState {
            temperature: 0.0,
            humidity: 0.0,
            dew_point: 0.0,
            battery: 0,
            last_seen: Instant::now(),
        }
    }
}

impl MiTempState {
    pub fn update(&mut self, json: &JsonValue) {
        self.last_seen = Instant::now();
        if let Some(temperature) = json["Temperature"]
            .as_number()
            .and_then(|num| f32::try_from(num).ok())
        {
            self.temperature = temperature;
        }
        if let Some(humidity) = json["Humidity"]
            .as_number()
            .and_then(|num| f32::try_from(num).ok())
        {
            self.humidity = humidity;
        }
        if let Some(battery) = json["Battery"]
            .as_number()
            .and_then(|num| u8::try_from(num).ok())
        {
            self.battery = battery;
        }
        if let Some(dew_point) = json["DewPoint"]
            .as_number()
            .and_then(|num| f32::try_from(num).ok())
        {
            self.dew_point = dew_point;
        }
    }
}

pub fn format_device_state<W: Write>(
    mut writer: W,
    device: &Device,
    state: &DeviceState,
) -> std::fmt::Result {
    if state.name.is_empty() {
        println!("{} has no name set, skipping", device.hostname);
        return Ok(());
    }
    writeln!(
        writer,
        "tasmota_online{{tasmota_id=\"{}\", name=\"{}\"}} 1",
        device.hostname, state.name
    )?;
    if let Some(switch_state) = state.state {
        writeln!(
            writer,
            "switch_state{{tasmota_id=\"{}\", name=\"{}\"}} {}",
            device.hostname,
            state.name,
            if switch_state { 1 } else { 0 }
        )?;
    }

    if let Some(power_watts) = state.power_watts {
        writeln!(
            writer,
            "power_watts{{tasmota_id=\"{}\", name=\"{}\"}} {}",
            device.hostname, state.name, power_watts
        )?;
    }

    if let Some(power_yesterday) = state.power_yesterday {
        writeln!(
            writer,
            "power_yesterday_kwh{{tasmota_id=\"{}\", name=\"{}\"}} {}",
            device.hostname, state.name, power_yesterday
        )?;
    }

    if let Some(power_today) = state.power_today {
        writeln!(
            writer,
            "power_today_kwh{{tasmota_id=\"{}\", name=\"{}\"}} {}",
            device.hostname, state.name, power_today
        )?;
    }

    if let Some(power_total) = state.power_total {
        writeln!(
            writer,
            "power_total_kwh{{tasmota_id=\"{}\", name=\"{}\"}} {}",
            device.hostname, state.name, power_total
        )?;
    }

    if let Some(power_total) = state.power_total_high {
        writeln!(
            writer,
            "power_total_high_kwh{{tasmota_id=\"{}\", name=\"{}\"}} {}",
            device.hostname, state.name, power_total
        )?;
    }

    if let Some(power_total) = state.power_total_low {
        writeln!(
            writer,
            "power_total_low_kwh{{tasmota_id=\"{}\", name=\"{}\"}} {}",
            device.hostname, state.name, power_total
        )?;
    }

    if let Some(gas_total) = state.gas_total {
        writeln!(
            writer,
            "gas_total_m3{{tasmota_id=\"{}\", name=\"{}\"}} {}",
            device.hostname, state.name, gas_total
        )?;
    }

    if let Some(co2) = state.co2 {
        writeln!(
            writer,
            "sensor_co2{{tasmota_id=\"{}\", name=\"{}\"}} {}",
            device.hostname, state.name, co2
        )?;
    }

    if let Some(pms) = state.pms_state.as_ref() {
        format_pms_state(&mut writer, device, state, pms)?;
    }

    if !state.firmware.is_empty() {
        writeln!(
            writer,
            r#"tasmota_version{{tasmota_id="{}", name="{}", firmware="{}", version="{}"}} 1"#,
            device.hostname, state.name, state.firmware, state.version
        )?;
    }

    Ok(())
}

pub fn format_mi_temp_state<W: Write>(
    mut writer: W,
    addr: BDAddr,
    names: &BTreeMap<BDAddr, String>,
    state: &MiTempState,
) -> std::fmt::Result {
    // sensor_battery{name="Living Room", mac="58:2D:34:39:1D:5B"} 100
    // sensor_temperature{name="Living Room", mac="58:2D:34:39:1D:5B"} 16.2
    // sensor_humidity{name="Living Room", mac="58:2D:34:39:1D:5B"} 61.

    let name = if let Some(name) = names.get(&addr) {
        name
    } else {
        return Ok(());
    };

    if state.battery > 0 {
        writeln!(
            writer,
            "sensor_battery{{mac=\"{}\", name=\"{}\"}} {}",
            addr, name, state.battery
        )?;
    }

    if state.temperature > 0.0 {
        writeln!(
            writer,
            "sensor_temperature{{mac=\"{}\", name=\"{}\"}} {}",
            addr, name, state.temperature
        )?;
    }

    if state.humidity > 0.0 {
        writeln!(
            writer,
            "sensor_humidity{{mac=\"{}\", name=\"{}\"}} {}",
            addr, name, state.humidity
        )?;
    }
    Ok(())
}

/// Stores the 6 byte address used to identify Bluetooth devices.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Hash, Eq, PartialEq, Default, Ord, PartialOrd)]
#[repr(C)]
pub struct BDAddr {
    pub address: [u8; 6usize],
}

impl BDAddr {
    /// parse BDAddr from the last 6 characters of the mac address
    /// first 6 characters are always set to 582D34
    pub fn from_mi_temp_mac_part(part: &str) -> Result<Self> {
        let bytes = ["58".as_bytes(), "2D".as_bytes(), "34".as_bytes()]
            .iter()
            .copied()
            .chain(part.as_bytes().chunks_exact(2))
            .map(|part: &[u8]| {
                let part = std::str::from_utf8(part)
                    .map_err(|_| Report::msg("Invalid mac address digit"))?;
                u8::from_str_radix(part, 16).wrap_err("Invalid mac address digit")
            })
            .collect::<Result<Vec<u8>>>()?;
        let mut address =
            <[u8; 6]>::try_from(bytes.as_slice()).wrap_err("Invalid mac address digit count")?;
        address.reverse();
        Ok(BDAddr { address })
    }
}

impl Display for BDAddr {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let a = self.address;
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            a[5], a[4], a[3], a[2], a[1], a[0]
        )
    }
}

impl Debug for BDAddr {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        (self as &dyn Display).fmt(f)
    }
}

//"PMS5003":{"CF1":6,"CF2.5":8,"CF10":8,"PM1":6,"PM2.5":8,"PM10":8,"PB0.3":0,"PB0.5":0,"PB1":0,"PB2.5":0,"PB5":0,"PB10":0}

#[derive(Debug, Clone, Default)]
pub struct PMSState {
    cf1: u16,
    cf2_5: u16,
    cf10: u16,
    pm1: u16,
    pm2_5: u16,
    pm10: u16,
    pb0_3: u16,
    pb0_5: u16,
    pb1: u16,
    pb2_5: u16,
    pb5: u16,
    pb10: u16,
}

impl PMSState {
    pub fn update(&mut self, json: &JsonValue) {
        if let Some(val) = json["CF1"]
            .as_number()
            .and_then(|num| u16::try_from(num).ok())
        {
            self.cf1 = val;
        }
        if let Some(val) = json["CF2.5"]
            .as_number()
            .and_then(|num| u16::try_from(num).ok())
        {
            self.cf2_5 = val;
        }
        if let Some(val) = json["CF10"]
            .as_number()
            .and_then(|num| u16::try_from(num).ok())
        {
            self.cf10 = val;
        }
        if let Some(val) = json["PM1"]
            .as_number()
            .and_then(|num| u16::try_from(num).ok())
        {
            self.pm1 = val;
        }
        if let Some(val) = json["PM2.5"]
            .as_number()
            .and_then(|num| u16::try_from(num).ok())
        {
            self.pm2_5 = val;
        }
        if let Some(val) = json["PM10"]
            .as_number()
            .and_then(|num| u16::try_from(num).ok())
        {
            self.pm10 = val;
        }
        if let Some(val) = json["PB0.3"]
            .as_number()
            .and_then(|num| u16::try_from(num).ok())
        {
            self.pb0_3 = val;
        }
        if let Some(val) = json["PB0.5"]
            .as_number()
            .and_then(|num| u16::try_from(num).ok())
        {
            self.pb0_5 = val;
        }
        if let Some(val) = json["PB1"]
            .as_number()
            .and_then(|num| u16::try_from(num).ok())
        {
            self.pb1 = val;
        }
        if let Some(val) = json["PB2.5"]
            .as_number()
            .and_then(|num| u16::try_from(num).ok())
        {
            self.pb2_5 = val;
        }
        if let Some(val) = json["PB5"]
            .as_number()
            .and_then(|num| u16::try_from(num).ok())
        {
            self.pb5 = val;
        }
        if let Some(val) = json["PB10"]
            .as_number()
            .and_then(|num| u16::try_from(num).ok())
        {
            self.pb10 = val;
        }
    }
}

pub fn format_pms_state<W: Write>(
    mut writer: W,
    device: &Device,
    device_state: &DeviceState,
    state: &PMSState,
) -> std::fmt::Result {
    let name = &device_state.name;

    writeln!(
        writer,
        "cf1{{tasmota_id=\"{}\", name=\"{}\"}} {}",
        device.hostname, name, state.cf1
    )?;
    writeln!(
        writer,
        "cf2_5{{tasmota_id=\"{}\", name=\"{}\"}} {}",
        device.hostname, name, state.cf2_5
    )?;
    writeln!(
        writer,
        "cf10{{tasmota_id=\"{}\", name=\"{}\"}} {}",
        device.hostname, name, state.cf10
    )?;
    writeln!(
        writer,
        "pm1{{tasmota_id=\"{}\", name=\"{}\"}} {}",
        device.hostname, name, state.pm1
    )?;
    writeln!(
        writer,
        "pm2_5{{tasmota_id=\"{}\", name=\"{}\"}} {}",
        device.hostname, name, state.pm2_5
    )?;
    writeln!(
        writer,
        "pm10{{tasmota_id=\"{}\", name=\"{}\"}} {}",
        device.hostname, name, state.pm10
    )?;
    writeln!(
        writer,
        "pb0_3{{tasmota_id=\"{}\", name=\"{}\"}} {}",
        device.hostname, name, state.pb0_3
    )?;
    writeln!(
        writer,
        "pb0_5{{tasmota_id=\"{}\", name=\"{}\"}} {}",
        device.hostname, name, state.pb0_5
    )?;
    writeln!(
        writer,
        "pb1{{tasmota_id=\"{}\", name=\"{}\"}} {}",
        device.hostname, name, state.pb1
    )?;
    writeln!(
        writer,
        "pb2_5{{tasmota_id=\"{}\", name=\"{}\"}} {}",
        device.hostname, name, state.pb2_5
    )?;
    writeln!(
        writer,
        "pb5{{tasmota_id=\"{}\", name=\"{}\"}} {}",
        device.hostname, name, state.pb5
    )?;
    writeln!(
        writer,
        "pb10{{tasmota_id=\"{}\", name=\"{}\"}} {}",
        device.hostname, name, state.pb10
    )?;
    Ok(())
}
