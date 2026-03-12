import { expect, test, type Page } from "@playwright/test";

const analyticsResponse = {
  generated_at: "2026-03-12 22:30:00",
  range: "24h",
  filter_options: {
    teams: ["team-a", "team-b"],
    models: ["gpt-4o", "claude-3-7-sonnet"],
    routers: ["default"],
    channels: ["openai", "bedrock"],
  },
  overview: {
    total_requests: 1280,
    total_tokens: 485000,
    input_tokens: 190000,
    output_tokens: 295000,
    avg_latency_ms: 182.4,
    success_rate: 99.1,
    delta: {
      total_requests: 12.5,
      total_tokens: 8.4,
      avg_latency_ms: -4.1,
      success_rate: 1.2,
    },
  },
  trend: {
    unit: "hour",
    points: [
      {
        bucket: "2026-03-12 20:00:00",
        label: "20:00",
        requests: 480,
        input_tokens: 74000,
        output_tokens: 122000,
        total_tokens: 196000,
        error_rate: 0.8,
        avg_latency_ms: 170.1,
        success_rate: 99.2,
      },
      {
        bucket: "2026-03-12 21:00:00",
        label: "21:00",
        requests: 800,
        input_tokens: 116000,
        output_tokens: 173000,
        total_tokens: 289000,
        error_rate: 1.1,
        avg_latency_ms: 190.7,
        success_rate: 98.9,
      },
    ],
  },
  topology: {
    nodes: [],
    links: [],
    flows: [
      {
        team_id: "team-a",
        router: "default",
        channel: "openai",
        model: "gpt-4o",
        requests: 680,
        total_tokens: 280000,
      },
    ],
    render_mode: "flow-summary",
  },
  team_usage: {
    leaderboard: [
      { team_id: "team-a", total_requests: 900, total_tokens: 320000 },
      { team_id: "team-b", total_requests: 380, total_tokens: 165000 },
    ],
    model_usage: [
      { team_id: "team-a", model: "gpt-4o", total_requests: 600, total_tokens: 220000 },
      { team_id: "team-a", model: "claude-3-7-sonnet", total_requests: 300, total_tokens: 100000 },
      { team_id: "team-b", model: "gpt-4o", total_requests: 380, total_tokens: 165000 },
    ],
  },
  system_reliability: {
    error_rate_trend: [
      {
        bucket: "2026-03-12 20:00:00",
        label: "20:00",
        requests: 480,
        input_tokens: 74000,
        output_tokens: 122000,
        total_tokens: 196000,
        error_rate: 0.8,
        avg_latency_ms: 170.1,
        success_rate: 99.2,
      },
    ],
    channel_latency: [
      { channel: "openai", total_requests: 910, avg_latency_ms: 168.3, p95_latency_ms: 244.1 },
      { channel: "bedrock", total_requests: 370, avg_latency_ms: 242.8, p95_latency_ms: 390.2 },
    ],
  },
  model_router: {
    model_share: [
      { name: "gpt-4o", requests: 980, total_tokens: 385000, percentage: 76.5 },
      { name: "claude-3-7-sonnet", requests: 300, total_tokens: 100000, percentage: 23.5 },
    ],
    router_summary: [{ name: "default", requests: 1280, total_tokens: 485000, percentage: 100 }],
    channel_summary: [
      { name: "openai", requests: 910, total_tokens: 350000, percentage: 71.1 },
      { name: "bedrock", requests: 370, total_tokens: 135000, percentage: 28.9 },
    ],
  },
  records_meta: {
    total: 25,
    latest_cursor: { id: 101, timestamp: "2026-03-12 22:29:00" },
  },
};

const firstPageRecords = {
  data: [
    {
      id: 101,
      timestamp: "2026-03-12 22:29:00",
      request_id: "req-101",
      team_id: "team-a",
      router: "default",
      matched_rule: "gpt-*",
      final_channel: "openai",
      channel: "openai",
      model: "gpt-4o",
      input_tokens: 120,
      output_tokens: 220,
      latency_ms: 180.4,
      fallback_triggered: false,
      status: "success",
      status_code: 200,
      error_message: null,
      provider_trace_id: null,
      provider_error_body: null,
    },
  ],
  total: 25,
  limit: 20,
  offset: 0,
  latest_cursor: { id: 101, timestamp: "2026-03-12 22:29:00" },
  new_records: 0,
};

const secondPageRecords = {
  data: [
    {
      id: 81,
      timestamp: "2026-03-12 21:11:00",
      request_id: "req-081",
      team_id: "team-b",
      router: "default",
      matched_rule: "claude-*",
      final_channel: "bedrock",
      channel: "bedrock",
      model: "claude-3-7-sonnet",
      input_tokens: 90,
      output_tokens: 40,
      latency_ms: 4200.4,
      fallback_triggered: false,
      status: "error",
      status_code: 502,
      error_message: "provider timeout",
      provider_trace_id: "trace-081",
      provider_error_body: "{\"error\":\"timeout\"}",
    },
  ],
  total: 25,
  limit: 20,
  offset: 20,
  latest_cursor: { id: 101, timestamp: "2026-03-12 22:29:00" },
  new_records: 0,
};

const exportRecords = {
  data: [...firstPageRecords.data, ...secondPageRecords.data],
  total: 2,
  limit: 100,
  offset: 0,
  latest_cursor: { id: 101, timestamp: "2026-03-12 22:29:00" },
  new_records: 0,
};

type DashboardMockOptions = {
  analytics?: typeof analyticsResponse;
  firstPage?: typeof firstPageRecords;
  secondPage?: typeof secondPageRecords;
  exportPage?: typeof exportRecords;
  invalidTokens?: string[];
  failAnalyticsCall?: number;
  failRecordsCall?: number;
};

type DashboardMockState = {
  analyticsRequests: URL[];
  recordRequests: URL[];
};

async function mockDashboardApis(
  page: Page,
  options: DashboardMockOptions = {}
): Promise<DashboardMockState> {
  const state: DashboardMockState = {
    analyticsRequests: [],
    recordRequests: [],
  };
  const invalidTokens = new Set(options.invalidTokens ?? ["invalid-key"]);
  let analyticsCallCount = 0;
  let recordsCallCount = 0;

  await page.route("**/api/dashboard/analytics?**", async (route) => {
    analyticsCallCount += 1;
    const authHeader = route.request().headers().authorization ?? "";
    const url = new URL(route.request().url());
    state.analyticsRequests.push(url);

    if (invalidTokens.has(authHeader.replace(/^Bearer\s+/, ""))) {
      await route.fulfill({
        status: 401,
        contentType: "application/json",
        body: JSON.stringify({ error: "unauthorized" }),
      });
      return;
    }

    if (options.failAnalyticsCall === analyticsCallCount) {
      await route.fulfill({
        status: 500,
        contentType: "application/json",
        body: JSON.stringify({ error: "server error" }),
      });
      return;
    }

    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(options.analytics ?? analyticsResponse),
    });
  });

  await page.route("**/api/dashboard/records?**", async (route) => {
    recordsCallCount += 1;
    const authHeader = route.request().headers().authorization ?? "";
    const url = new URL(route.request().url());
    state.recordRequests.push(url);

    if (invalidTokens.has(authHeader.replace(/^Bearer\s+/, ""))) {
      await route.fulfill({
        status: 401,
        contentType: "application/json",
        body: JSON.stringify({ error: "unauthorized" }),
      });
      return;
    }

    if (options.failRecordsCall === recordsCallCount) {
      await route.fulfill({
        status: 500,
        contentType: "application/json",
        body: JSON.stringify({ error: "server error" }),
      });
      return;
    }

    const offset = Number(url.searchParams.get("offset") ?? "0");
    const limit = Number(url.searchParams.get("limit") ?? "20");
    const sinceId = url.searchParams.get("since_id");

    let body =
      limit === 100
        ? (options.exportPage ?? exportRecords)
        : offset >= 20
          ? (options.secondPage ?? secondPageRecords)
          : (options.firstPage ?? firstPageRecords);

    if (sinceId === "101") {
      body = {
        ...(options.firstPage ?? firstPageRecords),
        data: (options.firstPage ?? firstPageRecords).data.slice(0, 1),
        limit: 1,
        new_records: 3,
      };
    }

    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(body),
    });
  });

  return state;
}

async function primeStoredToken(page: Page, token = "test-key") {
  await page.addInitScript((value) => {
    window.localStorage.setItem("apex-api-key", value);
  }, token);
}

async function primeClipboard(page: Page) {
  await page.addInitScript(() => {
    Object.defineProperty(window.navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: async () => undefined,
      },
    });
  });
}

async function primeExportCapture(page: Page) {
  await page.addInitScript(() => {
    const capture = {
      blob: null as Blob | null,
      filename: "",
      textPromise: Promise.resolve(""),
    };

    Object.defineProperty(window, "__downloadCapture", {
      configurable: true,
      value: capture,
      writable: true,
    });

    const originalCreateObjectURL = URL.createObjectURL.bind(URL);
    URL.createObjectURL = (object: Blob | MediaSource) => {
      if (object instanceof Blob) {
        capture.blob = object;
      }
      return originalCreateObjectURL(object);
    };

    const originalClick = HTMLAnchorElement.prototype.click;
    HTMLAnchorElement.prototype.click = function click() {
      capture.filename = this.download;
      capture.textPromise = capture.blob ? capture.blob.text() : Promise.resolve("");
      return originalClick.call(this);
    };
  });
}

async function selectFilterOption(page: Page, index: number, optionName: string) {
  await page.getByRole("combobox").nth(index).click();
  await page.getByRole("option", { name: optionName }).click();
}

test.describe("Dashboard page", () => {
  test("shows the embedded auth gate on root and validates empty submit", async ({ page }) => {
    await page.goto("/");

    await expect(page.getByRole("heading", { name: "Apex Gateway Dashboard" })).toBeVisible();
    await expect(page.getByPlaceholder("Enter API Key")).toBeVisible();
    await expect(page.getByRole("button", { name: "Open Dashboard" })).toBeVisible();

    await page.getByRole("button", { name: "Open Dashboard" }).click();
    await expect(page.getByText("Enter a global API key to continue.")).toBeVisible();
  });

  test("accepts token from the URL, scrubs it, and restores it to storage", async ({ page }) => {
    await mockDashboardApis(page);

    await page.goto("/dashboard?token=test-key");

    await expect(page.getByRole("button", { name: "Disconnect" })).toBeVisible({ timeout: 15000 });
    await expect(page.getByText("1,280")).toBeVisible();
    await expect(page.getByRole("tab", { name: "Overview" })).toBeVisible();
    await expect(page.getByText("Apex Gateway")).toBeVisible();
    await expect(page).toHaveURL(/\/dashboard\/?(\?range=24h&tab=overview)?$/);
    await expect
      .poll(() => page.evaluate(() => window.localStorage.getItem("apex-api-key")))
      .toBe("test-key");
  });

  test("removes invalid token from the URL and falls back to the connect view", async ({ page }) => {
    await mockDashboardApis(page);

    await page.goto("/dashboard?token=invalid-key");

    await expect(page.getByRole("heading", { name: "Apex Gateway Dashboard" })).toBeVisible();
    await expect(page.getByText("Invalid API key")).toBeVisible({ timeout: 15000 });
    await expect(page).toHaveURL(/\/dashboard\/\?range=24h&tab=overview$/);
    await expect
      .poll(() => page.evaluate(() => window.localStorage.getItem("apex-api-key")))
      .toBeNull();
  });

  test("restores a stored token and disconnects cleanly", async ({ page }) => {
    await mockDashboardApis(page);
    await primeStoredToken(page);

    await page.goto("/dashboard");

    await expect(page.getByRole("button", { name: "Disconnect" })).toBeVisible({ timeout: 15000 });
    await page.getByRole("button", { name: "Disconnect" }).click();

    await expect(page.getByRole("heading", { name: "Apex Gateway Dashboard" })).toBeVisible();
    await expect(page.getByText("Disconnected")).toBeVisible();
    await expect
      .poll(() => page.evaluate(() => window.localStorage.getItem("apex-api-key")))
      .toBeNull();
  });

  test("updates filters, tab state, and request params in the URL", async ({ page }) => {
    const state = await mockDashboardApis(page);
    await primeStoredToken(page);

    await page.goto("/dashboard?tab=records&offset=20");

    await expect(page.getByText("Page 2 of 2")).toBeVisible({ timeout: 15000 });
    await expect(page).toHaveURL(/tab=records&offset=20$/);

    await selectFilterOption(page, 0, "Last 7 Days");
    await selectFilterOption(page, 1, "team-b");
    await selectFilterOption(page, 2, "claude-3-7-sonnet");

    await expect(page).toHaveURL(
      /\/dashboard\/\?range=7d&tab=records&team_id=team-b&model=claude-3-7-sonnet$/
    );
    await expect(page.getByText("Page 1 of 2")).toBeVisible();
    await expect
      .poll(() => state.analyticsRequests.at(-1)?.searchParams.toString())
      .toContain("range=7d");
    await expect
      .poll(() => state.analyticsRequests.at(-1)?.searchParams.toString())
      .toContain("team_id=team-b");
    await expect
      .poll(() => state.analyticsRequests.at(-1)?.searchParams.toString())
      .toContain("model=claude-3-7-sonnet");
    await expect
      .poll(() => state.recordRequests.at(-1)?.searchParams.get("offset"))
      .toBe("0");
  });

  test("renders every dashboard tab after bootstrap", async ({ page }) => {
    await mockDashboardApis(page);
    await primeStoredToken(page);

    await page.goto("/dashboard");

    await expect(page.getByText("Global Trend")).toBeVisible({ timeout: 15000 });
    await expect(page.getByText("Traffic Topology")).toBeVisible();

    await page.getByRole("tab", { name: "Team & Usage" }).click();
    await expect(page.getByText("Team Leaderboard")).toBeVisible();
    await expect(page.getByText("Model Usage by Team")).toBeVisible();

    await page.getByRole("tab", { name: "System & Reliability" }).click();
    await expect(page.getByText("Error Rate Trend")).toBeVisible();
    await expect(page.getByText("Channel Latency Comparison")).toBeVisible();

    await page.getByRole("tab", { name: "Model & Router" }).click();
    await expect(page.getByText("Model Share")).toBeVisible();
    await expect(page.getByText("Router Summary")).toBeVisible();
    await expect(page.getByText("Channel Summary")).toBeVisible();

    await page.getByRole("tab", { name: "Records" }).click();
    await expect(page.getByText("Raw Usage Records")).toBeVisible();
  });

  test("supports record copy feedback and details drawer interactions", async ({ page }) => {
    await mockDashboardApis(page);
    await primeStoredToken(page);
    await primeClipboard(page);

    await page.goto("/dashboard?tab=records");

    await expect(page.getByText("Raw Usage Records")).toBeVisible({ timeout: 15000 });
    await page.getByRole("button", { name: "req-101" }).click();
    await expect(page.getByText("Copied")).toBeVisible();

    await page.getByRole("row", { name: /req-101/ }).click();
    await expect(page.getByText("Request Details")).toBeVisible();
    await expect(page.getByRole("heading", { name: "req-101" })).toBeVisible();
    await expect(page.getByText("Routing & Status")).toBeVisible();
    await expect(page.getByText("in 120 / out 220")).toBeVisible();

    await page.getByRole("button", { name: "Close details drawer" }).click();
    await expect(page.getByText("Request Details")).toBeHidden();
  });

  test("shows a new records banner when refreshing from a later page", async ({ page }) => {
    await mockDashboardApis(page);
    await primeStoredToken(page);

    await page.goto("/dashboard");
    await page.getByRole("tab", { name: "Records" }).click();
    await page.getByRole("button", { name: "Next" }).click();
    await expect(page.getByText("Page 2 of 2")).toBeVisible();

    await page.getByRole("button", { name: "Refresh" }).click();
    await expect(
      page.getByText("3 new records available. Jump back to page 1 to inspect them.")
    ).toBeVisible();

    await page.getByRole("button", { name: "View Latest" }).click();
    await expect(page.getByText("Page 1 of 2")).toBeVisible();
  });

  test("shows a refresh error banner while preserving the last successful snapshot", async ({
    page,
  }) => {
    await mockDashboardApis(page, { failAnalyticsCall: 2 });
    await primeStoredToken(page);

    await page.goto("/dashboard?tab=records");

    await expect(page.getByText("Raw Usage Records")).toBeVisible({ timeout: 15000 });
    await expect(page.getByText("req-101")).toBeVisible();

    await page.getByRole("button", { name: "Refresh" }).click();

    await expect(
      page.getByText("Refresh failed. Showing the most recent successful snapshot.")
    ).toBeVisible();
    await expect(page.getByText("req-101")).toBeVisible();
  });

  test("exports the current records as CSV", async ({ page }) => {
    await mockDashboardApis(page);
    await primeStoredToken(page);
    await primeExportCapture(page);

    await page.goto("/dashboard?tab=records");

    await expect(page.getByText("Raw Usage Records")).toBeVisible({ timeout: 15000 });
    await page.getByRole("button", { name: "Export CSV" }).click();

    await expect
      .poll(() =>
        page.evaluate(() => {
          const capture = (window as Window & {
            __downloadCapture?: {
              filename: string;
            };
          }).__downloadCapture;
          return capture?.filename ?? "";
        })
      )
      .toMatch(/^apex-dashboard-records-.*\.csv$/);

    const result = await page.evaluate(async () => {
      const capture = (window as Window & {
        __downloadCapture: {
          filename: string;
          textPromise: Promise<string>;
        };
      }).__downloadCapture;

      return {
        filename: capture.filename,
        text: await capture.textPromise,
      };
    });

    expect(result.filename).toMatch(/^apex-dashboard-records-.*\.csv$/);
    expect(result.text).toContain("timestamp,request_id,team_id,router");
    expect(result.text).toContain("req-101");
    expect(result.text).toContain("req-081");
  });
});
