use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BTreeMap<String, CostUnit>")]
pub struct CostUnits(BTreeMap<String, CostUnit>);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CostUnit {
    pub one: String,
    pub other: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<CostCurrency>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostCurrency {
    pub code: String,
    pub scale: u8,
}

impl TryFrom<BTreeMap<String, CostUnit>> for CostUnits {
    type Error = String;
    fn try_from(units: BTreeMap<String, CostUnit>) -> Result<Self, String> {
        let label = |value: &str| {
            !value.is_empty()
                && value.len() <= 48
                && value.trim() == value
                && value.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, ' ' | '-' | '_')
                })
        };
        if units.len() > 32
            || units.iter().any(|(key, value)| {
                !crate::blueprint_step::token(key)
                    || !label(&value.one)
                    || !label(&value.other)
                    || value.currency.as_ref().is_some_and(|currency| {
                        currency.scale > 15
                            || currency.code.len() != 3
                            || !currency
                                .code
                                .bytes()
                                .all(|character| character.is_ascii_uppercase())
                    })
            })
        {
            return Err("invalid static cost unit display declaration".into());
        }
        Ok(Self(units))
    }
}

impl CostUnits {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn format(&self, amount: u64, unit: &str) -> Result<String, String> {
        if amount > crate::blueprint_step::MAX_COST_AMOUNT || !crate::blueprint_step::token(unit) {
            return Err("invalid bounded cost amount".into());
        }
        let Some(display) = self.0.get(unit) else {
            return Ok(format!("{amount} units ({unit})"));
        };
        if let Some(currency) = &display.currency {
            let scale = usize::from(currency.scale);
            let digits = format!("{amount:0width$}", width = scale + 1);
            let decimal = if scale == 0 {
                digits
            } else {
                let (whole, fraction) = digits.split_at(digits.len() - scale);
                let mut fraction = fraction.to_string();
                while fraction.len() > scale.min(2) && fraction.ends_with('0') {
                    fraction.pop();
                }
                format!("{whole}.{fraction}")
            };
            return Ok(format!("{} {decimal}", currency.code));
        }
        Ok(format!(
            "{amount} {}",
            if amount == 1 {
                &display.one
            } else {
                &display.other
            }
        ))
    }
}

#[cfg(test)]
mod tests;
