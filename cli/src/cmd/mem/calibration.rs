use super::{mem_get_request, mem_post_request};
use crate::MemCalibrationAction;

pub(crate) fn handle_mem_calibration(
    http: &reqwest::blocking::Client,
    base: &str,
    action: MemCalibrationAction,
) -> Result<serde_json::Value, String> {
    match action {
        MemCalibrationAction::Log {
            since,
            limit,
            count,
        } => {
            let mut url = format!("{}/api/mem/calibration/log", base);
            let mut sep = '?';
            if let Some(s) = &since {
                url.push_str(&format!("{}since={}", sep, urlencoding::encode(s)));
                sep = '&';
            }
            if let Some(l) = &limit {
                url.push_str(&format!("{}limit={}", sep, l));
                sep = '&';
            }
            if count {
                url.push(sep);
                url.push_str("count=true");
            }
            mem_get_request(http, &url)
        }
        MemCalibrationAction::Sweep { force } => {
            let url = if force {
                format!("{}/api/mem/calibration/sweep?force=true", base)
            } else {
                format!("{}/api/mem/calibration/sweep", base)
            };
            mem_post_request(http, &url, &serde_json::json!({}))
        }
    }
}
