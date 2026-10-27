const catalogPrice = (price_id: string, amount: string) => ({ price_id, amount, currency_code: "USD", list_price: amount, discount: "0" });
export const desktopCatalogFixture = {
  status: "available", catalog: { schema_version: 1, mode: "test", tiers: [
    { tier: "free", limits: { workspaces: 2, teams: 6, agent_tabs: 35, mesh_machines: 2, mem_items: 500, neighbors: 0 }, prices: { monthly: null, yearly: null } },
    { tier: "pro", limits: { workspaces: 7, teams: 10, agent_tabs: 70, mesh_machines: 5, mem_items: 1500, neighbors: 5 }, prices: { monthly: catalogPrice("pri_pro", "999"), yearly: catalogPrice("pri_pro_year", "7200") } },
    { tier: "max", limits: { workspaces: 20, teams: null, agent_tabs: null, mesh_machines: 12, mem_items: 5000, neighbors: 20 }, prices: { monthly: catalogPrice("pri_max", "2499"), yearly: catalogPrice("pri_max_year", "18000") } },
  ], trial_offer: { tier: "pro", price_id: "pri_pro", days: 7, eligible: true } },
};
