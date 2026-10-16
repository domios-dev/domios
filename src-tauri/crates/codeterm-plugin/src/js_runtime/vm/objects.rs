use rquickjs::{Array, Ctx, IntoJs, Object, Value};

pub(super) fn arguments<'js>(
    ctx: &Ctx<'js>,
    values: &[serde_json::Value],
) -> rquickjs::Result<Array<'js>> {
    let array = Array::new(ctx.clone())?;
    for (index, value) in values.iter().enumerate() {
        array.set(index, convert(ctx, value)?)?;
    }
    Ok(array)
}

fn convert<'js>(ctx: &Ctx<'js>, value: &serde_json::Value) -> rquickjs::Result<Value<'js>> {
    match value {
        serde_json::Value::Null => Ok(Value::new_null(ctx.clone())),
        serde_json::Value::Bool(value) => value.into_js(ctx),
        serde_json::Value::Number(value) => {
            value.as_f64().ok_or(rquickjs::Error::Unknown)?.into_js(ctx)
        }
        serde_json::Value::String(value) => value.as_str().into_js(ctx),
        serde_json::Value::Array(values) => arguments(ctx, values)?.into_js(ctx),
        serde_json::Value::Object(values) => {
            let object = Object::new(ctx.clone())?;
            for (key, value) in values {
                object.prop(
                    key.as_str(),
                    rquickjs::object::Property::from(convert(ctx, value)?)
                        .writable()
                        .enumerable()
                        .configurable(),
                )?;
            }
            object.into_js(ctx)
        }
    }
}
