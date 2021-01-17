use color_eyre::{eyre::WrapErr, Report, Result};
use json::JsonValue;
use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::fmt::{self, Debug, Display, Formatter, Write};

#[derive(Debug, Eq, PartialEq, Clone, Hash)]
pub struct Device {
    pub topic: String,
    pub hostname: String,
}

impl Device {
    pub fn get_topic(&self, prefix: &str, command: &str) -> String {
        format!("{}/{}/{}/{}", prefix, self.topic, self.hostname, command)
    }
}

#[derive(Debug, Default)]
pub struct DeviceState {
    pub state: Option<bool>,
    pub name: String,
    pub power_watts: Option<f32>,
    pub power_yesterday: Option<f32>,
    pub power_today: Option<f32>,
    pub co2: Option<f32>,
    pub mi_temp_devices: BTreeMap<BDAddr, MiTempState>,
    pub pms_state: Option<PMSState>,
}

impl DeviceState {
    pub fn update(&mut self, json: JsonValue) {
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
        if let Some(today) = json["MHZ19B"]["CarbonDioxide"]
            .as_number()
            .and_then(|num| f32::try_from(num).ok())
        {
            self.co2 = Some(today);
        }

        if json["PMS5003"].is_object() {
            let pms = self.pms_state.get_or_insert(PMSState::default());
            pms.update(&json["PMS5003"]);
        }

        for (key, value) in json.entries() {
            if let Some(addr) = key.strip_prefix("MJ_HT_V1-") {
                match BDAddr::from_mi_temp_mac_part(addr) {
                    Ok(addr) => {
                        let state = self.mi_temp_devices.entry(addr).or_default();
                        state.update(value);
                    }
                    Err(e) => eprintln!("Failed to parse mitemp mac: {:#}", e),
                }
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct MiTempState {
    temperature: f32,
    humidity: f32,
    dew_point: f32,
    battery: u8,
}

impl MiTempState {
    pub fn update(&mut self, json: &JsonValue) {
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
    mi_temp_names: &BTreeMap<BDAddr, String>,
) -> std::fmt::Result {
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

    if let Some(co2) = state.co2 {
        writeln!(
            writer,
            "sensor_co2{{tasmota_id=\"{}\", name=\"{}\"}} {}",
            device.hostname, state.name, co2
        )?;
    }

    for (addr, state) in state.mi_temp_devices.iter() {
        format_mi_temp_state(&mut writer, device, *addr, mi_temp_names, state)?;
    }

    if let Some(pms) = state.pms_state.as_ref() {
        format_pms_state(&mut writer, device, state, pms)?;
    }

    Ok(())
}

pub fn format_mi_temp_state<W: Write>(
    mut writer: W,
    device: &Device,
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
            "sensor_battery{{tasmota_id=\"{}\", mac=\"{}\", name=\"{}\"}} {}",
            device.hostname, addr, name, state.battery
        )?;
    }

    if state.temperature > 0.0 {
        writeln!(
            writer,
            "sensor_temperature{{tasmota_id=\"{}\", mac=\"{}\", name=\"{}\"}} {}",
            device.hostname, addr, name, state.temperature
        )?;
    }

    if state.humidity > 0.0 {
        writeln!(
            writer,
            "sensor_humidity{{tasmota_id=\"{}\", mac=\"{}\", name=\"{}\"}} {}",
            device.hostname, addr, name, state.humidity
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
