import { expect, test } from "@playwright/test";

const shouldRunRealDashboardTests = process.env.RUN_REAL_DASHBOARD_TESTS === "true";
const dashboardApiKey = process.env.DASHBOARD_API_KEY ?? "sk-dashboard-admin-key";

test.describe("Dashboard page with real backend", () => {
  test.beforeEach(async ({ page }) => {
    test.skip(
      !shouldRunRealDashboardTests,
      "Real backend dashboard tests require RUN_REAL_DASHBOARD_TESTS=true"
    );

    await page.addInitScript((apiKey) => {
      window.localStorage.setItem("apex-api-key", apiKey);
    }, dashboardApiKey);
  });

  test("renders seeded analytics across dashboard tabs", async ({ page }) => {
    await page.goto("/dashboard?range=24h&tab=overview");

    await expect(page.getByText("Connected")).toBeVisible({ timeout: 15000 });
    await expect(page.getByText("Control Plane")).toBeVisible();
    await expect(page.getByText("25")).toBeVisible();
    await expect(page.getByText("Traffic Topology")).toBeVisible();

    await page.getByRole("tab", { name: "Team & Usage" }).click();
    await expect(page.getByText("Team Leaderboard")).toBeVisible();
    await expect(page.getByText("team-alpha").first()).toBeVisible();
    await expect(page.getByText("team-beta").first()).toBeVisible();

    await page.getByRole("tab", { name: "System & Reliability" }).click();
    await expect(page.getByText("Error Rate Trend")).toBeVisible();
    await expect(page.getByText("Channel Latency Comparison")).toBeVisible();

    await page.getByRole("tab", { name: "Model & Router" }).click();
    await expect(page.getByText("Model Share")).toBeVisible();
    await expect(page.getByText("Router Summary")).toBeVisible();
    await expect(page.getByText("Channel Summary")).toBeVisible();
  });

  test("supports records pagination, filters, and details drawer", async ({ page }) => {
    await page.goto("/dashboard?range=24h&tab=records");

    await expect(page.getByText("Raw Usage Records")).toBeVisible({ timeout: 15000 });
    await expect(page.getByText("Page 1 of 2 · 25 total records")).toBeVisible();
    await expect(page.getByText("req-live-000")).toBeVisible();

    await page.getByRole("button", { name: "Next" }).click();
    await expect(page.getByText("Page 2 of 2 · 25 total records")).toBeVisible();

    await page.getByRole("combobox").nth(1).click();
    await page.getByRole("option", { name: "team-beta" }).click();
    await page.getByRole("combobox").nth(2).click();
    await page.getByRole("option", { name: "claude-3-7-sonnet" }).click();

    await expect(page).toHaveURL(
      /\/dashboard\/\?range=24h&tab=records&team_id=team-beta&model=claude-3-7-sonnet$/
    );
    await expect(page.getByText("Page 1 of 1 · 6 total records")).toBeVisible();
    await expect(page.getByText("req-live-015")).toBeVisible();

    await page.getByRole("row", { name: /req-live-015/ }).click();
    await expect(page.getByText("Request Details")).toBeVisible();
    await expect(page.getByRole("heading", { name: "req-live-015" })).toBeVisible();
    await expect(page.getByText("Routing & Status")).toBeVisible();
    await expect(page.getByText("bedrock", { exact: true })).toBeVisible();
  });
});
