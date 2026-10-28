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
    pub name: Option<String>,
}

pub(super) fn parse(args: &[String], setting: bool) -> Result<MetricsArgs, String> {
    let mut out = MetricsArgs {
        viewport: None,
        restore: false,
        metrics: false,
        full: false,
        name: None,
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
            "--mobile" => mobile = true,
            "--restore" => out.restore = true,
            "--metrics-json" => out.metrics = true,
            "--full" => out.full = true,
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
    Ok(out)
}

impl BrowserRuntime {
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
        let (base64, metrics) = if let Some(viewport) = options.viewport.filter(|_| options.restore)
        {
            let shot = self
                .host
                .capture_with_viewport(viewport, options.full)
                .await?;
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
            let metrics = self.send(&page, "Page.getLayoutMetrics", json!({})).await?;
            (self.screenshot_base64(&page, options.full).await?, metrics)
        };
        let name = super::inbox::evidence_file_name(
            options.name.as_deref().unwrap_or(""),
            &format!("shot-{}", super::calls::now_ms()),
        );
        let path = self.save_evidence(ctx, name, base64).await?;
        if options.metrics {
            Ok(json!({"path":path.to_string_lossy(),"layoutWidth":metrics.pointer("/cssLayoutViewport/clientWidth"),"contentWidth":metrics.pointer("/cssContentSize/width"),"metrics":metrics,"restored":options.restore}).to_string())
        } else {
            Ok(format!(
                "saved {} (show it with @ds-image {})",
                path.display(),
                path.display()
            ))
        }
    }
}

#[cfg(test)]
mod tests;
