import { act } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SupporterAllowanceRow } from "./SupporterAllowanceRow";
import { API, harness, statusFixture } from "../Supporter/testSupport";

vi.mock("../../lib/webApi", async (importOriginal) => ({
  ...await importOriginal<typeof import("../../lib/webApi")>(),
  daemonFetch: (_api: unknown, path: string, init: RequestInit) => fetch(path, init),
  daemonAuthToken: () => "fixture",
}));
let setup: ReturnType<typeof harness>;
let status = statusFixture();
beforeEach(() => {
  setup = harness();
  status = statusFixture();
  vi.spyOn(globalThis, "fetch").mockImplementation(async () => new Response(JSON.stringify(status), { status: 200 }));
});
afterEach(async () => setup.cleanup());

const estimate = (host: HTMLElement, period: string) => host.querySelector(`[data-estimate="${period}"]`);

describe("Supporter allowance", () => {
  it("allowance_row_shows_spend_estimates_and_request_counts", async () => {
    const host = await setup.render(<SupporterAllowanceRow api={API} />);
    expect(estimate(host, "today")?.getAttribute("data-usd")).toBe("0.125");
    expect(estimate(host, "month")?.getAttribute("data-usd")).toBe("0.75");
    expect(host.querySelectorAll("[data-estimate]")).toHaveLength(2);
    expect(host.querySelector('[role="status"]')?.getAttribute("data-cap")).toBe("none");
  });
  it("allowance_row_unpriced_adapter_requests_only", async () => {
    status.pricing = null;
    const host = await setup.render(<SupporterAllowanceRow api={API} />);
    expect(host.querySelectorAll("[data-estimate]")).toHaveLength(0);
    expect(host.querySelector('[role="status"]')?.getAttribute("data-requests-hour")).toBe("4");
    expect(host.querySelector('[role="status"]')?.getAttribute("data-requests-day")).toBe("12");
  });
  it("allowance_row_local_adapter_without_requests_is_hidden", async () => {
    status.pricing = { display_name: "Local", input_usd_per_million: 0, output_usd_per_million: 0 };
    status.calls_last_hour = 0;
    status.calls_last_day = 0;
    const host = await setup.render(<SupporterAllowanceRow api={API} />);
    expect(host.childElementCount).toBe(0);
  });
  it("allowance_row_refreshes_on_journal_event", async () => {
    const host = await setup.render(<SupporterAllowanceRow api={API} />);
    status.spend.today.estimated_usd = 0.9;
    await setup.emit();
    expect(estimate(host, "today")?.getAttribute("data-usd")).toBe("0.9");
    await setup.cleanup();
    expect(setup.listeners.size).toBe(0);
  });
  it("allowance_rows_share_one_status_read_and_subscription", async () => {
    const host = await setup.render(<><SupporterAllowanceRow api={API} /><SupporterAllowanceRow api={API} /></>);
    expect(host.querySelectorAll('[role="status"]')).toHaveLength(2);
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(setup.listeners.size).toBe(1);
    await setup.emit();
    expect(fetch).toHaveBeenCalledTimes(2);
  });
  it("allowance_row_local_adapter_with_requests_has_no_money", async () => {
    status.pricing = { display_name: "Local", input_usd_per_million: 0, output_usd_per_million: 0 };
    const host = await setup.render(<SupporterAllowanceRow api={API} />);
    expect(host.querySelectorAll("[data-estimate]")).toHaveLength(0);
    expect(host.querySelector('[role="status"]')?.getAttribute("data-requests-hour")).toBe("4");
  });
  it("allowance_row_coalesces_inflight_journal_events", async () => {
    let resolve!: (response: Response) => void;
    const pending = new Promise<Response>((next) => { resolve = next; });
    vi.mocked(fetch).mockReturnValueOnce(pending);
    const host = await setup.render(<SupporterAllowanceRow api={API} />);
    await setup.emit();
    await setup.emit();
    expect(fetch).toHaveBeenCalledTimes(1);
    status.spend.today.estimated_usd = 1.5;
    await act(async () => { resolve(new Response(JSON.stringify(statusFixture()), { status: 200 })); });
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(estimate(host, "today")?.getAttribute("data-usd")).toBe("1.5");
  });

});
