use crate::device::{Device, DsmrMessageType};

#[derive(Debug, Eq, PartialEq)]
pub enum Topic {
    Lwt(Device),
    Power(Device),
    State(Device),
    Sensor(Device),
    Result(Device),
    Other(String),
    Status(Device),
    Msg(Device),
    Water(Device),
    Gas(Device),
    Energy1(Device),
    Energy2(Device),
    DsmrPower(Device),
    Rtl(Device, String),
}

impl Topic {
    pub fn dsmr_type(&self) -> Option<DsmrMessageType> {
        match self {
            Topic::Water(_) => Some(DsmrMessageType::Water),
            Topic::Gas(_) => Some(DsmrMessageType::Gas),
            Topic::Energy1(_) => Some(DsmrMessageType::Energy1),
            Topic::Energy2(_) => Some(DsmrMessageType::Energy2),
            Topic::DsmrPower(_) => Some(DsmrMessageType::Power),
            _ => None,
        }
    }

    pub fn into_device(self) -> Device {
        match self {
            Topic::Lwt(device) => device,
            Topic::Power(device) => device,
            Topic::State(device) => device,
            Topic::Sensor(device) => device,
            Topic::Result(device) => device,
            Topic::Other(device) => Device { hostname: device },
            Topic::Status(device) => device,
            Topic::Msg(device) => device,
            Topic::Water(device) => device,
            Topic::Gas(device) => device,
            Topic::Energy1(device) => device,
            Topic::Energy2(device) => device,
            Topic::DsmrPower(device) => device,
            Topic::Rtl(device, _) => device,
        }
    }
}

impl From<&str> for Topic {
    fn from(raw: &str) -> Self {
        if let Some(rf_name) = raw.strip_suffix("/msg") {
            let device = Device {
                hostname: rf_name.to_string(),
            };
            return Topic::Msg(device);
        }
        if let Some((device, topic)) = raw
            .strip_prefix("rtl_433/")
            .and_then(|topic| topic.split_once('/'))
        {
            let device = Device {
                hostname: device.to_string(),
            };
            return Topic::Rtl(device, topic.into());
        }
        if let Some(name) = raw.strip_suffix("/water") {
            let device = Device {
                hostname: name.to_string(),
            };
            return Topic::Water(device);
        }
        if let Some(name) = raw.strip_suffix("/gas_delivered") {
            let device = Device {
                hostname: name.to_string(),
            };
            return Topic::Gas(device);
        }
        if let Some(name) = raw.strip_suffix("/energy_delivered_tariff1") {
            let device = Device {
                hostname: name.to_string(),
            };
            return Topic::Energy1(device);
        }
        if let Some(name) = raw.strip_suffix("/energy_delivered_tariff2") {
            let device = Device {
                hostname: name.to_string(),
            };
            return Topic::Energy2(device);
        }
        if let Some(name) = raw.strip_suffix("/power_delivered_l1") {
            let device = Device {
                hostname: name.to_string(),
            };
            return Topic::DsmrPower(device);
        }

        let mut parts = raw.split('/');
        if let (Some(prefix), Some(hostname), Some(cmd)) =
            (parts.next(), parts.next(), parts.next())
        {
            let device = Device {
                hostname: hostname.to_string(),
            };
            match (prefix, cmd) {
                ("tele", "LWT") => Topic::Lwt(device),
                ("tele", "STATE") => Topic::State(device),
                ("stat", "POWER") => Topic::Power(device),
                ("tele", "SENSOR") => Topic::Sensor(device),
                ("stat", "RESULT") => Topic::Result(device),
                ("stat", "STATUS") => Topic::Status(device),
                ("stat", "STATUS2") => Topic::Status(device),
                _ => Topic::Other(raw.to_string()),
            }
        } else {
            Topic::Other(raw.to_string())
        }
    }
}

#[test]
fn parse_topic() {
    let device = Device {
        hostname: "hostname".to_string(),
    };
    assert_eq!(Topic::Lwt(device.clone()), Topic::from("tele/hostname/LWT"));
    assert_eq!(
        Topic::Power(device.clone()),
        Topic::from("stat/hostname/POWER")
    );
    assert_eq!(
        Topic::State(device.clone()),
        Topic::from("tele/hostname/STATE")
    );
    assert_eq!(
        Topic::Sensor(device.clone()),
        Topic::from("tele/hostname/SENSOR")
    );
    assert_eq!(Topic::Result(device), Topic::from("stat/hostname/RESULT"));
}
