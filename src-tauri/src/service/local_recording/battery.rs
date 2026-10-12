use crate::service::ServiceContext;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TransmitterBattery {
    pub slot: u8,
    pub connected: bool,
    pub percent: Option<u8>,
}

pub(crate) fn decode_status(report: &[u8]) -> Result<Vec<TransmitterBattery>, String> {
    if report.len() != 64 || report[..5] != [0x55, 0xbb, 0xdd, 0x10, 0x80] {
        return Err("recording_battery_invalid_frame".into());
    }
    let length = u16::from_be_bytes([report[5], report[6]]) as usize;
    if !(4..=56).contains(&length)
        || report[7..9].iter().any(|value| *value > 1)
        || report[9..11].iter().any(|value| *value > 100)
    {
        return Err("recording_battery_invalid_payload".into());
    }
    Ok((0..2)
        .map(|i| TransmitterBattery {
            slot: i as u8 + 1,
            connected: report[7 + i] == 1,
            percent: (report[7 + i] == 1).then_some(report[9 + i]),
        })
        .collect())
}

impl ServiceContext {
    pub(crate) async fn local_recording_battery(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<TransmitterBattery>, String> {
        let status = self.local_recording_status(plugin_id)?;
        if !status.granted || !status.enabled || !status.available {
            return Ok(Vec::new());
        }
        let (devices, generation) = self
            .local_recording_registry
            .selected_devices(plugin_id)
            .ok_or("devices_unavailable")?;
        let trigger = self
            .local_recording_registry
            .declaration(plugin_id)
            .ok_or("local_recording_not_registered")?;
        let result = tokio::task::spawn_blocking(move || {
            #[cfg(windows)]
            {
                super::platform::windows::battery::read(&trigger, &devices.input_id)
            }
            #[cfg(not(windows))]
            {
                let _ = (devices, trigger);
                Ok::<_, String>(Vec::new())
            }
        })
        .await
        .map_err(|_| "recording_battery_query_failed")??;
        let current = self.local_recording_status(plugin_id)?;
        if !current.granted
            || !current.enabled
            || !current.available
            || current.generation != generation
        {
            return Err("local_recording_configuration_changed".into());
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
