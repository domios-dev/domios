const catalogPrice = (price_id: string, amount: string) => ({ price_id, amount, currency_code: "USD", list_price: amount, discount: "0" });
const providerPrice = (price_id: string, plan: string, amount: string, interval: "month" | "year") => ({
  price_id, plan, product_id: `pro_${plan}`, product_name: plan,
  unit_price: { amount, currency_code: "USD" }, billing_cycle: { interval, frequency: 1 }, trial: { kind: "none" }, localized_prices: [],
});
export const desktopCatalogFixture = {
  status: "available", catalog: { schema_version: 1, mode: "test", prices: [
    providerPrice("pri_pro", "pro", "999", "month"), providerPrice("pri_pro_year", "pro", "7200", "year"),
    providerPrice("pri_max", "max", "2499", "month"), providerPrice("pri_max_year", "max", "18000", "year"),
    providerPrice("pri_sponsor", "sponsor", "7500", "month"), providerPrice("pri_sponsor_year", "sponsor", "90000", "year"),
  ], tiers: [
    { tier: "free", limits: { workspaces: 2, teams: 6, agent_tabs: 35, mesh_machines: 2, mem_items: 500, neighbors: 0 }, prices: { monthly: null, yearly: null } },
    { tier: "pro", limits: { workspaces: 7, teams: 10, agent_tabs: 70, mesh_machines: 5, mem_items: 1500, neighbors: 5 }, prices: { monthly: catalogPrice("pri_pro", "999"), yearly: catalogPrice("pri_pro_year", "7200") } },
    { tier: "max", limits: { workspaces: 20, teams: null, agent_tabs: null, mesh_machines: 12, mem_items: 5000, neighbors: 20 }, prices: { monthly: catalogPrice("pri_max", "2499"), yearly: catalogPrice("pri_max_year", "18000") } },
    { tier: "sponsor", limits: { workspaces: null, teams: null, agent_tabs: null, mesh_machines: null, mem_items: null, neighbors: null }, prices: { monthly: catalogPrice("pri_sponsor", "7500"), yearly: catalogPrice("pri_sponsor_year", "90000") } },
  ], trial_offer: { tier: "pro", price_id: "pri_pro", days: 7, eligible: true } },
};
