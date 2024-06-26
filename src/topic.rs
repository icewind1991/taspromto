use crate::device::Device;

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
}

impl From<&str> for Topic {
    fn from(raw: &str) -> Self {
        if let Some(rf_name) = raw.strip_suffix("/msg") {
            let device = Device {
                hostname: rf_name.to_string(),
            };
            return Topic::Msg(device);
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
