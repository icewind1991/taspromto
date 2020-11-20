use std::fmt::Write;

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
    pub state: bool,
    pub name: String,
    pub power_watts: Option<f32>,
    pub power_yesterday: Option<f32>,
    pub power_today: Option<f32>,
}

pub fn format_device_state<W: Write>(
    mut writer: W,
    device: &Device,
    state: &DeviceState,
) -> Result<(), std::fmt::Error> {
    writeln!(
        writer,
        "switch_state{{tasmota_id=\"{}\", name=\"{}\"}} {}",
        device.hostname,
        state.name,
        if state.state { 1 } else { 0 }
    )?;

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
    Ok(())
}
