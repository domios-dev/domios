use crate::jsonl_window::{ordered::OrderedValue, records::RecordWindow};
use rquickjs::{Array, Ctx, IntoJs, Object, Value};

pub(super) fn record_arguments<'js>(
    ctx: &Ctx<'js>,
    values: &[serde_json::Value],
    window: Option<&RecordWindow>,
) -> rquickjs::Result<Array<'js>> {
    let array = arguments(ctx, values)?;
    if let Some(window) = window {
        let records: Array = array.get(usize::from(values.len() == 3))?;
        for (index, record) in window.records.iter().enumerate() {
            if let Some(value) = &record.ordered_value {
                let object: Object = records.get(index)?;
                property(&object, "value", ordered(ctx, value)?)?;
            }
        }
    }
    Ok(array)
}

fn property<'js>(object: &Object<'js>, key: &str, value: Value<'js>) -> rquickjs::Result<()> {
    object.prop(
        key,
        rquickjs::object::Property::from(value)
            .writable()
            .enumerable()
            .configurable(),
    )
}

fn ordered<'js>(ctx: &Ctx<'js>, value: &OrderedValue) -> rquickjs::Result<Value<'js>> {
    match value {
        OrderedValue::Scalar(value) => convert(ctx, value),
        OrderedValue::Array(values) => {
            let array = Array::new(ctx.clone())?;
            for (index, value) in values.iter().enumerate() {
                array.set(index, ordered(ctx, value)?)?;
            }
            array.into_js(ctx)
        }
        OrderedValue::Object(values) => {
            let object = Object::new(ctx.clone())?;
            for (key, value) in values {
                property(&object, key, ordered(ctx, value)?)?;
            }
            object.into_js(ctx)
        }
    }
}

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
                property(&object, key, convert(ctx, value)?)?;
            }
            object.into_js(ctx)
        }
    }
}
