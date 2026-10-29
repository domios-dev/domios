export interface CostUnit { one:string; other:string; currency?:{code:string; scale:number} }
export type CostUnits = Record<string,CostUnit>;

export function formatCostAmount(amount:number, unit:string, units:CostUnits = {}):string {
  const token = (value:string) => /^[a-z][a-z0-9_-]{0,63}$/.test(value);
  const label = (value:unknown):value is string => typeof value === "string" && value.length > 0 && value.length <= 48 && value.trim() === value && /^[A-Za-z0-9 _-]+$/.test(value);
  if (!Number.isSafeInteger(amount) || amount < 0 || !token(unit) || !units || typeof units !== "object" || Array.isArray(units) || Object.keys(units).length > 32) throw Error("Invalid bounded cost display");
  for (const [key,value] of Object.entries(units)) {
    if (!token(key) || !value || typeof value !== "object" || !label(value.one) || !label(value.other)
      || Object.keys(value).some(field => !["one","other","currency"].includes(field))) throw Error("Invalid cost unit declaration");
    const currency = value.currency;
    if (currency !== undefined && (!currency || typeof currency !== "object" || !/^[A-Z]{3}$/.test(currency.code)
      || !Number.isInteger(currency.scale) || currency.scale < 0 || currency.scale > 15
      || Object.keys(currency).some(field => !["code","scale"].includes(field)))) throw Error("Invalid cost currency declaration");
  }
  const display = Object.prototype.hasOwnProperty.call(units,unit) ? units[unit] : undefined;
  if (!display) return `${amount} units (${unit})`;
  if (!display.currency) return `${amount} ${amount === 1 ? display.one : display.other}`;
  const {code,scale} = display.currency;
  const digits = String(amount).padStart(scale+1,"0");
  let fraction = scale === 0 ? "" : digits.slice(-scale);
  while (fraction.length > Math.min(scale,2) && fraction.endsWith("0")) fraction = fraction.slice(0,-1);
  return `${code} ${scale === 0 ? digits : `${digits.slice(0,-scale)}.${fraction}`}`;
}
