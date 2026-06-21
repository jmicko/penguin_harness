use clap::ValueEnum;
use serde::de::{Error as DeError, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Real,
    Isolated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowId(pub u32);

impl WindowId {
    pub fn as_hex(self) -> String {
        format!("0x{:x}", self.0)
    }
}

impl fmt::Display for WindowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_hex())
    }
}

impl Serialize for WindowId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(self.0)
    }
}

impl<'de> Deserialize<'de> for WindowId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct WindowIdVisitor;

        impl Visitor<'_> for WindowIdVisitor {
            type Value = WindowId;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a decimal window id, or a 0x-prefixed hex window id string")
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: DeError,
            {
                let id = u32::try_from(value).map_err(E::custom)?;
                Ok(WindowId(id))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
            where
                E: DeError,
            {
                let id = u32::try_from(value).map_err(E::custom)?;
                Ok(WindowId(id))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: DeError,
            {
                value.parse().map_err(E::custom)
            }
        }

        deserializer.deserialize_any(WindowIdVisitor)
    }
}

impl FromStr for WindowId {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        let id = if let Some(hex) = trimmed
            .strip_prefix("0x")
            .or_else(|| trimmed.strip_prefix("0X"))
        {
            u32::from_str_radix(hex, 16)?
        } else {
            trimmed.parse::<u32>()?
        };
        Ok(Self(id))
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WindowInfo {
    pub id: WindowId,
    pub desktop: Option<i32>,
    pub pid: Option<u32>,
    pub geometry: Geometry,
    pub host: Option<String>,
    pub title: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    #[default]
    Left,
    Middle,
    Right,
}

impl MouseButton {
    pub fn xtest_button(self) -> u8 {
        match self {
            Self::Left => 1,
            Self::Middle => 2,
            Self::Right => 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WindowId;

    #[test]
    fn window_id_deserializes_decimal_and_hex_strings() {
        let decimal: WindowId = serde_json::from_str("100663310").unwrap();
        let hex: WindowId = serde_json::from_str("\"0x600000e\"").unwrap();

        assert_eq!(decimal, WindowId(100663310));
        assert_eq!(hex, WindowId(100663310));
    }
}
