use super::BrowserRuntime;
use crate::surface::runtime::SurfaceCallCtx;
use codeterm_browser::viewport::{clamp, Viewport, ViewportDecision, ViewportRequest};
use serde_json::json;

#[derive(Debug)]
pub(super) struct MetricsArgs {
    pub viewport: Option<Viewport>,
    pub restore: bool,
    pub metrics: bool,
    pub full: bool,
    pub include_overlay: bool,
    pub name: Option<String>,
    pub region: Option<[f64; 4]>,
    pub scale: Option<f64>,
}

pub(super) fn parse(args: &[String], setting: bool) -> Result<MetricsArgs, String> {
    let mut out = MetricsArgs {
        viewport: None,
        restore: false,
        metrics: false,
        full: false,
        include_overlay: false,
        name: None,
        region: None,
        scale: None,
    };
    let mut size = None;
    let mut mobile = false;
    let mut dpr = 1.0;
    let mut i = 0;
    if setting {
        if args.first().map(String::as_str) != Some("set") || args.len() < 3 {
            return Err("usage: viewport set <w> <h> [--mobile] [--dpr N]".into());
        }
        size = Some((args[1].clone(), args[2].clone()));
        i = 3;
    }
    while i < args.len() {
        match args[i].as_str() {
            "--scale" if !setting && out.scale.is_none() => {
                i += 1;
                out.scale = Some(
                    args.get(i)
                        .and_then(|s| s.parse::<f64>().ok())
                        .filter(|v| v.is_finite() && (0.1..=4.0).contains(v))
                        .ok_or("scale must be 0.1 to 4")?,
                );
            }
            "--region" if !setting && out.region.is_none() => {
                i += 1;
                let values: Vec<_> = args
                    .get(i)
                    .ok_or("region requires x,y,width,height")?
                    .split(',')
                    .map(str::parse::<f64>)
                    .collect::<Result<_, _>>()
                    .map_err(|_| "invalid region")?;
                let region: [f64; 4] = values
                    .try_into()
                    .map_err(|_| "region requires x,y,width,height")?;
                if region
                    .iter()
                    .any(|n| !n.is_finite() || *n < 0.0 || *n > 100_000.0)
                    || region[2] == 0.0
                    || region[3] == 0.0
                {
                    return Err("invalid region".into());
                }
                out.region = Some(region);
            }
            "--mobile" => mobile = true,
            "--restore" => out.restore = true,
            "--metrics-json" => out.metrics = true,
            "--full" => out.full = true,
            "--include-overlay" if !setting => out.include_overlay = true,
            "--dpr" => {
                i += 1;
                dpr = args
                    .get(i)
                    .ok_or("--dpr needs a number")?
                    .parse::<f64>()
                    .map_err(|_| "invalid DPR")?;
            }
            "--viewport" => {
                i += 1;
                let (w, h) = args
                    .get(i)
                    .and_then(|v| v.split_once('x'))
                    .ok_or("--viewport needs <w>x<h>")?;
                size = Some((w.into(), h.into()));
            }
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            name if !setting && out.name.is_none() => out.name = Some(name.into()),
            _ => return Err("unexpected viewport argument".into()),
        }
        i += 1;
    }
    if let Some((w, h)) = size {
        let width = w.parse::<f64>().map_err(|_| "invalid width")?;
        let height = h.parse::<f64>().map_err(|_| "invalid height")?;
        if !dpr.is_finite() || dpr <= 0.0 {
            return Err("invalid DPR".into());
        }
        let mut viewport =
            clamp(ViewportRequest { width, height, dpr }).ok_or("invalid viewport")?;
        viewport.mobile = mobile;
        out.viewport = Some(viewport);
    } else if mobile || dpr != 1.0 || out.restore {
        return Err("device options require --viewport".into());
    }
    if (out.region.is_some() || out.scale.is_some()) && out.viewport.is_some() {
        return Err("region/scale capture cannot combine with device viewport options".into());
    }
    if out.region.is_some() && out.full {
        return Err("region cannot combine with full capture".into());
    }
    if let Some([_, _, w, h]) = out.region {
        if w * h * out.scale.unwrap_or(1.0).powi(2) > 16_777_216.0 {
            return Err("region exceeds capture pixel limit".into());
        }
    }
    Ok(out)
}

impl BrowserRuntime {
    pub(super) async fn screenshot_metrics(&self, page: &codeterm_browser::PageChannel) -> Result<serde_json::Value, String> {
        let mut metrics = self.send(page, "Page.getLayoutMetrics", json!({})).await?;
        metrics["devicePixelRatio"] = self.evaluate(page, "window.devicePixelRatio", false).await?;
        Ok(metrics)
    }

    async fn apply_metrics(&self, viewport: Viewport) -> Result<(), String> {
        match self
            .host
            .set_device_viewport(
                ViewportRequest {
                    width: viewport.width as f64,
                    height: viewport.height as f64,
                    dpr: viewport.dpr,
                },
                viewport.mobile,
            )
            .await?
        {
            ViewportDecision::Invalid => Err("invalid_viewport".into()),
            ViewportDecision::RefusedAttached => {
                Err("refused_attached: device metrics require an owned target".into())
            }
            _ => Ok(()),
        }
    }

    pub(super) async fn viewport_verb(&self, args: &[String]) -> Result<String, String> {
        let viewport = parse(args, true)?.viewport.ok_or("missing viewport")?;
        self.apply_metrics(viewport).await?;
        Ok(json!({"width":viewport.width,"height":viewport.height,"dpr":viewport.dpr,"mobile":viewport.mobile}).to_string())
    }

    pub(super) async fn screenshot_verb(
        &self,
        ctx: &SurfaceCallCtx,
        args: &[String],
    ) -> Result<String, String> {
        let options = parse(args, false)?;
        let page = self.ensure_page().await?;
        let (base64, metrics) = if options.region.is_some() || options.scale.is_some() {
            let metrics = self.screenshot_metrics(&page).await?;
            let bounds = if options.full {
                &metrics["cssContentSize"]
            } else {
                &metrics["cssLayoutViewport"]
            };
            let [x, y, width, height] = options.region.unwrap_or([
                bounds["pageX"].as_f64().unwrap_or(0.0),
                bounds["pageY"].as_f64().unwrap_or(0.0),
                bounds[if options.full { "width" } else { "clientWidth" }]
                    .as_f64()
                    .ok_or("missing capture width")?,
                bounds[if options.full {
                    "height"
                } else {
                    "clientHeight"
                }]
                .as_f64()
                .ok_or("missing capture height")?,
            ]);
            let scale = options.scale.unwrap_or(1.0);
            if width * height * scale.powi(2) > 16_777_216.0 {
                return Err("capture exceeds pixel limit".into());
            }
            let shot = self.capture_screenshot(&page,json!({"format":"png","captureBeyondViewport":true,"clip":{"x":x,"y":y,"width":width,"height":height,"scale":scale}}),options.include_overlay).await?;
            (
                shot["data"]
                    .as_str()
                    .ok_or("screenshot returned no image")?
                    .to_string(),
                metrics,
            )
        } else if let Some(viewport) = options.viewport.filter(|_| options.restore) {
            if !options.include_overlay {
                self.overlay_capture(&page, true).await?;
            }
            let shot = self
                .host
                .capture_with_viewport(viewport, options.full)
                .await;
            let restored = if !options.include_overlay {
                self.overlay_capture(&page, false).await
            } else {
                Ok(())
            };
            let shot = shot?;
            restored?;
            (
                shot["data"]
                    .as_str()
                    .ok_or("screenshot returned no image")?
                    .to_string(),
                shot["metrics"].clone(),
            )
        } else {
            if let Some(viewport) = options.viewport {
                self.apply_metrics(viewport).await?;
            }
            let metrics = self.screenshot_metrics(&page).await?;
            (
                self.screenshot_base64(&page, options.full, options.include_overlay)
                    .await?,
                metrics,
            )
        };
        let name = super::inbox::evidence_file_name(
            options.name.as_deref().unwrap_or(""),
            &format!("shot-{}", super::calls::now_ms()),
        );
        let path = self.save_evidence(ctx, name, base64).await?;
        let viewport = capture_viewport(&metrics)?;
        if options.metrics {
            Ok(json!({"path":path.to_string_lossy(),"viewport":viewport,"layoutWidth":metrics.pointer("/cssLayoutViewport/clientWidth"),"contentWidth":metrics.pointer("/cssContentSize/width"),"metrics":metrics,"restored":options.restore}).to_string())
        } else {
            Ok(format!(
                "saved {} (CSS viewport {}x{}, DPR {}; show it with <ds-image src=\"{}\" />)",
                path.display(),
                viewport["width"], viewport["height"], viewport["dpr"],
                path.to_string_lossy()
                    .replace('&', "&amp;")
                    .replace('"', "&quot;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
            ))
        }
    }
}

pub(super) fn capture_viewport(metrics: &serde_json::Value) -> Result<serde_json::Value, String> {
    let width = metrics.pointer("/cssLayoutViewport/clientWidth").and_then(serde_json::Value::as_f64).ok_or("missing CSS viewport width")?;
    let height = metrics.pointer("/cssLayoutViewport/clientHeight").and_then(serde_json::Value::as_f64).ok_or("missing CSS viewport height")?;
    let dpr = metrics["devicePixelRatio"].as_f64().filter(|n|n.is_finite() && *n > 0.0).ok_or("missing capture DPR")?;
    Ok(json!({"width":width,"height":height,"dpr":dpr}))
}

#[cfg(test)]
mod tests;
