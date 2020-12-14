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
    pub mi_temp_devices: BTreeMap<BDAddr, MiTempState>,
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

    for (addr, state) in state.mi_temp_devices.iter() {
        format_mi_temp_state(&mut writer, *addr, mi_temp_names, state)?;
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
