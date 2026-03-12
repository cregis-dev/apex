import { test, expect, type Page } from "@playwright/test";

const metricsResponse = {
  total_requests: 1280,
  total_errors: 12,
  total_fallbacks: 4,
  avg_latency_ms: 182.4,
  error_rate: 0.9375,
  p95_latency_ms: 240.6,
};

const trendsResponse = {
  period: "daily",
  data: [
    {
      date: "2026-03-08",
      requests: 100,
      input_tokens: 1000,
      output_tokens: 2000,
      total_errors: 2,
      total_fallbacks: 1,
      avg_latency_ms: 120.4,
      p95_latency_ms: 180.2,
    },
    {
      date: "2026-03-09",
      requests: 220,
      input_tokens: 2000,
      output_tokens: 3000,
      total_errors: 4,
      total_fallbacks: 2,
      avg_latency_ms: 210.8,
      p95_latency_ms: 320.9,
    },
  ],
};

const rankingsResponse = {
  by: "team_id",
  data: [
    { name: "team-a", requests: 500, input_tokens: 1000, output_tokens: 2000, percentage: 60 },
    { name: "team-b", requests: 330, input_tokens: 900, output_tokens: 1400, percentage: 40 },
  ],
};

const usageResponse = {
  data: [
    {
      id: 1,
      timestamp: "2026-03-10 10:00:00",
      request_id: "req-123",
      team_id: "team-a",
      router: "default",
      channel: "openai",
      model: "gpt-4o",
      input_tokens: 120,
      output_tokens: 240,
      latency_ms: 156.2,
      fallback_triggered: false,
      status: "success",
      status_code: 200,
      error_message: null,
      provider_trace_id: null,
      provider_error_body: null,
    },
  ],
  total: 1,
  limit: 20,
  offset: 0,
};

const errorUsageResponse = {
  data: [
    {
      id: 2,
      timestamp: "2026-03-09 11:15:00",
      request_id: "req-500",
      team_id: "team-b",
      router: "default",
      channel: "openai",
      model: "gpt-4o",
      input_tokens: 90,
      output_tokens: 40,
      latency_ms: 420.4,
      fallback_triggered: false,
      status: "error",
      status_code: 502,
      error_message: "provider timeout",
      provider_trace_id: "trace-500",
      provider_error_body: "{\"error\":\"timeout\"}",
    },
  ],
  total: 1,
  limit: 20,
  offset: 0,
};

const trendDayUsageResponse = {
  data: [
    {
      id: 3,
      timestamp: "2026-03-09 08:45:00",
      request_id: "req-321",
      team_id: "team-a",
      router: "default",
      channel: "openai",
      model: "gpt-4o-mini",
      input_tokens: 60,
      output_tokens: 75,
      latency_ms: 318.3,
      fallback_triggered: true,
      status: "fallback",
      status_code: 200,
      error_message: null,
      provider_trace_id: null,
      provider_error_body: null,
    },
  ],
  total: 1,
  limit: 20,
  offset: 0,
};

async function mockDashboardApis(page: Page) {
  await page.route("**/api/metrics", async (route) => {
    const authHeader = route.request().headers().authorization;

    if (authHeader === "Bearer invalid-key") {
      await route.fulfill({ status: 401, body: JSON.stringify({ error: "unauthorized" }) });
      return;
    }

    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(metricsResponse),
    });
  });

  await page.route("**/api/metrics/trends?**", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(trendsResponse),
    });
  });

  await page.route("**/api/metrics/rankings?**", async (route) => {
    const url = new URL(route.request().url());
    const by = url.searchParams.get("by") ?? "team_id";

    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ ...rankingsResponse, by }),
    });
  });

  await page.route("**/api/usage?**", async (route) => {
    const url = new URL(route.request().url());
    const status = url.searchParams.get("status");
    const startDate = url.searchParams.get("start_date");
    const endDate = url.searchParams.get("end_date");

    let body = usageResponse;
    if (status === "errors") {
      body = errorUsageResponse;
    } else if (startDate === "2026-03-09" && endDate === "2026-03-09") {
      body = trendDayUsageResponse;
    }

    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(body),
    });
  });
}

test.describe("Dashboard page", () => {
  test("shows the dashboard auth gate on root instead of a separate home page", async ({ page }) => {
    await page.goto("/");

    await expect(page.getByRole("heading", { name: "Apex Gateway Dashboard" })).toBeVisible();
    await expect(page.getByPlaceholder("Enter API Key")).toBeVisible();
    await expect(page.getByRole("button", { name: "Open Dashboard" })).toBeVisible();
  });

  test("accepts auth_token from the URL and loads dashboard content", async ({ page }) => {
    await mockDashboardApis(page);

    await page.goto("/dashboard?auth_token=test-key");

    await expect(page.getByRole("heading", { name: "Apex Gateway Dashboard" })).toBeVisible({ timeout: 15000 });
    await expect(page.getByText("Connected")).toBeVisible({ timeout: 15000 });
    await expect(page.getByText("1,280")).toBeVisible();
    await expect(page.getByText("Usage Records", { exact: true })).toBeVisible();
    await expect(page).toHaveURL(/\/dashboard\/?$/);
  });

  test("normalizes /dashboard/index.html auth links and still loads dashboard content", async ({ page }) => {
    await mockDashboardApis(page);

    await page.goto("/dashboard/index.html?auth_token=test-key");

    await expect(page.getByRole("heading", { name: "Apex Gateway Dashboard" })).toBeVisible({ timeout: 15000 });
    await expect(page.getByText("Connected")).toBeVisible({ timeout: 15000 });
    await expect(page.getByText("1,280")).toBeVisible();
    await expect(page).toHaveURL(/\/dashboard\/$/);
  });

  test("loads dashboard with a stored API key", async ({ page }) => {
    await mockDashboardApis(page);
    await page.addInitScript(() => {
      localStorage.setItem("apex-api-key", "test-key");
    });

    await page.goto("/dashboard");

    await expect(page.getByRole("button", { name: "Disconnect" })).toBeVisible({ timeout: 15000 });
    await expect(page.getByText("team-a")).toBeVisible();
    await expect(page.getByRole("button", { name: "This Week" })).toBeVisible();
  });

  test("shows a validation message for an invalid API key", async ({ page }) => {
    await mockDashboardApis(page);
    await page.goto("/dashboard");

    await page.getByPlaceholder("Enter API Key").fill("invalid-key");
    await page.getByRole("button", { name: "Open Dashboard" }).click();

    await expect(page.getByText("Invalid API Key")).toBeVisible();
    await expect(page.getByPlaceholder("Enter API Key")).toBeVisible();
  });

  test("disconnect clears stored auth and returns to the auth gate", async ({ page }) => {
    await mockDashboardApis(page);
    await page.addInitScript(() => {
      localStorage.setItem("apex-api-key", "test-key");
    });

    await page.goto("/dashboard");
    await page.getByRole("button", { name: "Disconnect" }).click();

    await expect(page.getByRole("button", { name: "Open Dashboard" })).toBeVisible();

    const storedKey = await page.evaluate(() => localStorage.getItem("apex-api-key"));
    expect(storedKey).toBeNull();
  });

  test("clicking the error rate KPI drills the usage table into error records", async ({ page }) => {
    await mockDashboardApis(page);
    await page.goto("/dashboard?auth_token=test-key");

    await page.getByRole("button", { name: "Inspect usage records from Error Rate" }).click();

    await expect(page.getByText("Drilldown active")).toBeVisible();
    await expect(page.getByText("Error Rate · Status Errors")).toBeVisible();
    await expect(page.getByText("Status: Errors")).toBeVisible();
    await expect(page.getByText("req-500")).toBeVisible();
    await expect(page).toHaveURL(/status=errors/);
  });

  test("clicking a trend point narrows the usage table to that exact day", async ({ page }) => {
    await mockDashboardApis(page);
    await page.goto("/dashboard?auth_token=test-key");

    await page.getByLabel("Inspect request volume records for 2026-03-09").click();

    await expect(page.getByText("Drilldown active")).toBeVisible();
    await expect(page.getByText("request volume spike · Date 2026-03-09")).toBeVisible();
    await expect(page.getByText("req-321")).toBeVisible();
    await expect(page).toHaveURL(/range=custom/);
    await expect(page).toHaveURL(/start_date=2026-03-09/);
    await expect(page).toHaveURL(/end_date=2026-03-09/);
  });
});
