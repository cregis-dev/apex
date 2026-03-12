"use client";

import {
  FormEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { AlertCircle, LoaderCircle, PlugZap } from "lucide-react";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import Image from "next/image";

import apexLogo from "../../../apex.svg";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { DashboardFilters } from "@/components/dashboard/dashboard-filters";
import { ModelRouterTab } from "@/components/dashboard/model-router-tab";
import { OverviewTab } from "@/components/dashboard/overview-tab";
import { RecordsTab } from "@/components/dashboard/records-tab";
import { SystemReliabilityTab } from "@/components/dashboard/system-reliability-tab";
import { TeamUsageTab } from "@/components/dashboard/team-usage-tab";
import type {
  DashboardAnalyticsResponse,
  DashboardRange,
  DashboardRecordsResponse,
  DashboardTab,
  UsageRecord,
} from "@/components/dashboard/types";
import { UsageDetailsDrawer } from "@/components/dashboard/usage-details-drawer";

const API_KEY_STORAGE_KEY = "apex-api-key";
const DASHBOARD_PATH = "/dashboard/";

function normalizeDashboardPath(pathname: string | null): string {
  if (!pathname || pathname === "/dashboard" || pathname === "/dashboard/index.html") {
    return DASHBOARD_PATH;
  }

  return pathname;
}

function defaultFilterOptions(): DashboardAnalyticsResponse["filter_options"] {
  return {
    teams: [],
    models: [],
    routers: [],
    channels: [],
  };
}

function buildCsv(records: UsageRecord[]) {
  const headers = [
    "timestamp",
    "request_id",
    "team_id",
    "router",
    "matched_rule",
    "final_channel",
    "model",
    "input_tokens",
    "output_tokens",
    "latency_ms",
    "status",
    "status_code",
    "error_message",
    "provider_trace_id",
    "provider_error_body",
  ];

  const escape = (value: string | number | null | undefined) => {
    const normalized = value == null ? "" : String(value);
    if (/[",\n]/.test(normalized)) {
      return `"${normalized.replace(/"/g, '""')}"`;
    }
    return normalized;
  };

  const rows = records.map((record) =>
    [
      record.timestamp,
      record.request_id ?? "",
      record.team_id,
      record.router,
      record.matched_rule ?? "",
      record.final_channel,
      record.model,
      record.input_tokens,
      record.output_tokens,
      record.latency_ms ?? "",
      record.status,
      record.status_code ?? "",
      record.error_message ?? "",
      record.provider_trace_id ?? "",
      record.provider_error_body ?? "",
    ]
      .map(escape)
      .join(",")
  );

  return [headers.join(","), ...rows].join("\n");
}

function downloadCsvFile(content: string) {
  const blob = new Blob([content], { type: "text/csv;charset=utf-8;" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = `apex-dashboard-records-${new Date().toISOString().replace(/[:.]/g, "-")}.csv`;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}

export default function DashboardClient() {
  const router = useRouter();
  const pathname = usePathname();
  const searchParams = useSearchParams();
  const initialized = useRef(false);

  const [bootstrapping, setBootstrapping] = useState(true);
  const [connecting, setConnecting] = useState(false);
  const [dashboardLoading, setDashboardLoading] = useState(false);
  const [recordsLoading, setRecordsLoading] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [authToken, setAuthToken] = useState("");
  const [authInput, setAuthInput] = useState("");
  const [authMessage, setAuthMessage] = useState("");
  const [dashboardError, setDashboardError] = useState("");
  const [analytics, setAnalytics] = useState<DashboardAnalyticsResponse | null>(null);
  const [records, setRecords] = useState<UsageRecord[]>([]);
  const [filterOptions, setFilterOptions] = useState(defaultFilterOptions);
  const [range, setRange] = useState<DashboardRange>("24h");
  const [teamId, setTeamId] = useState("all");
  const [model, setModel] = useState("all");
  const [activeTab, setActiveTab] = useState<DashboardTab>("overview");
  const [limit] = useState(20);
  const [offset, setOffset] = useState(0);
  const [totalRecords, setTotalRecords] = useState(0);
  const [latestCursor, setLatestCursor] =
    useState<DashboardRecordsResponse["latest_cursor"]>(null);
  const [pendingNewRecords, setPendingNewRecords] = useState(0);
  const [selectedRecord, setSelectedRecord] = useState<UsageRecord | null>(null);
  const [copiedRequestId, setCopiedRequestId] = useState<string | null>(null);

  const apiBaseUrl = useMemo(() => {
    if (typeof window === "undefined") {
      return process.env.NEXT_PUBLIC_API_URL ?? "";
    }

    return process.env.NEXT_PUBLIC_API_URL ?? window.location.origin;
  }, []);

  const syncUrlState = useCallback(
    (next: {
      range: DashboardRange;
      teamId: string;
      model: string;
      tab: DashboardTab;
      offset: number;
    }) => {
      const params = new URLSearchParams();
      params.set("range", next.range);
      params.set("tab", next.tab);
      if (next.teamId !== "all") {
        params.set("team_id", next.teamId);
      }
      if (next.model !== "all") {
        params.set("model", next.model);
      }
      if (next.offset > 0) {
        params.set("offset", String(next.offset));
      }

      const query = params.toString();
      const nextPath = query
        ? `${normalizeDashboardPath(pathname)}?${query}`
        : normalizeDashboardPath(pathname);

      if (typeof window !== "undefined") {
        window.history.replaceState({}, "", nextPath);
        return;
      }

      router.replace(nextPath, { scroll: false });
    },
    [pathname, router]
  );

  const clearAuth = useCallback((message = "") => {
    if (typeof window !== "undefined") {
      localStorage.removeItem(API_KEY_STORAGE_KEY);
    }
    setAuthToken("");
    setAuthInput("");
    setAuthMessage(message);
    setDashboardError("");
    setAnalytics(null);
    setRecords([]);
    setTotalRecords(0);
    setLatestCursor(null);
    setPendingNewRecords(0);
    setSelectedRecord(null);
    setDashboardLoading(false);
    setRecordsLoading(false);
    setRefreshing(false);
  }, []);

  const buildApiUrl = useCallback(
    (path: string, params?: URLSearchParams) =>
      `${apiBaseUrl}${path}${params ? `?${params.toString()}` : ""}`,
    [apiBaseUrl]
  );

  const buildAuthHeaders = useCallback(
    () => ({
      Authorization: `Bearer ${authToken}`,
    }),
    [authToken]
  );

  const buildSharedParams = useCallback(
    (nextOffset = offset) => {
      const params = new URLSearchParams();
      params.set("range", range);
      params.set("limit", String(limit));
      params.set("offset", String(nextOffset));
      if (teamId !== "all") {
        params.set("team_id", teamId);
      }
      if (model !== "all") {
        params.set("model", model);
      }
      return params;
    },
    [limit, model, offset, range, teamId]
  );

  const fetchWithAuth = useCallback(
    async <T,>(url: string) => {
      const response = await fetch(url, {
        headers: buildAuthHeaders(),
      });

      if (response.status === 401 || response.status === 403) {
        clearAuth("Invalid API key");
        throw new Error("AUTH_INVALID");
      }

      if (!response.ok) {
        throw new Error(`Request failed with status ${response.status}`);
      }

      return (await response.json()) as T;
    },
    [buildAuthHeaders, clearAuth]
  );

  const loadAnalytics = useCallback(async () => {
    const data = await fetchWithAuth<DashboardAnalyticsResponse>(
      buildApiUrl("/api/dashboard/analytics", buildSharedParams())
    );
    setAnalytics(data);
    setFilterOptions(data.filter_options);
    return data;
  }, [buildApiUrl, buildSharedParams, fetchWithAuth]);

  const loadRecords = useCallback(
    async (nextOffset = offset) => {
      const params = buildSharedParams(nextOffset);
      const response = await fetchWithAuth<DashboardRecordsResponse>(
        buildApiUrl("/api/dashboard/records", params)
      );
      setRecords(response.data);
      setTotalRecords(response.total);
      setLatestCursor(response.latest_cursor);
      if (nextOffset === 0) {
        setPendingNewRecords(0);
      }
      return response;
    },
    [buildApiUrl, buildSharedParams, fetchWithAuth, offset]
  );

  useEffect(() => {
    if (initialized.current) {
      return;
    }

    initialized.current = true;

    const queryToken = searchParams.get("token")?.trim() ?? "";
    const storedToken =
      typeof window !== "undefined"
        ? localStorage.getItem(API_KEY_STORAGE_KEY)?.trim() ?? ""
        : "";
    const nextToken = queryToken || storedToken;
    const nextRange = (searchParams.get("range")?.trim() ?? "24h") as DashboardRange;
    const nextTab = (searchParams.get("tab")?.trim() ?? "overview") as DashboardTab;
    const nextTeamId = searchParams.get("team_id")?.trim() ?? "all";
    const nextModel = searchParams.get("model")?.trim() ?? "all";
    const nextOffset = Number(searchParams.get("offset") ?? "0");

    if (nextRange === "1h" || nextRange === "24h" || nextRange === "7d" || nextRange === "30d") {
      setRange(nextRange);
    }
    if (nextTab === "overview" || nextTab === "team" || nextTab === "system" || nextTab === "model" || nextTab === "records") {
      setActiveTab(nextTab);
    }
    setTeamId(nextTeamId || "all");
    setModel(nextModel || "all");
    setOffset(Number.isFinite(nextOffset) ? Math.max(0, nextOffset) : 0);

    if (queryToken && typeof window !== "undefined") {
      localStorage.setItem(API_KEY_STORAGE_KEY, queryToken);
      const params = new URLSearchParams(searchParams.toString());
      params.delete("token");
      const query = params.toString();
      const nextPath = query
        ? `${normalizeDashboardPath(pathname)}?${query}`
        : normalizeDashboardPath(pathname);
      window.history.replaceState({}, "", nextPath);
    }

    setAuthToken(nextToken);
    setAuthInput(nextToken);
    setBootstrapping(false);
  }, [pathname, searchParams]);

  useEffect(() => {
    if (bootstrapping || !authToken) {
      return;
    }

    let cancelled = false;

    const run = async () => {
      setDashboardLoading(true);
      setRecordsLoading(true);
      setDashboardError("");
      try {
        const [analyticsResponse, recordsResponse] = await Promise.all([
          loadAnalytics(),
          loadRecords(offset),
        ]);
        if (cancelled) {
          return;
        }
        setFilterOptions(analyticsResponse.filter_options);
        setLatestCursor(recordsResponse.latest_cursor);
      } catch (error) {
        if (cancelled || (error as Error).message === "AUTH_INVALID") {
          return;
        }
        setDashboardError("Failed to sync dashboard data.");
      } finally {
        if (!cancelled) {
          setDashboardLoading(false);
          setRecordsLoading(false);
        }
      }
    };

    void run();

    return () => {
      cancelled = true;
    };
  }, [authToken, bootstrapping, loadAnalytics, loadRecords, offset, range, teamId, model]);

  useEffect(() => {
    if (bootstrapping) {
      return;
    }

    syncUrlState({ range, teamId, model, tab: activeTab, offset });
  }, [activeTab, bootstrapping, model, offset, range, syncUrlState, teamId]);

  const handleConnect = useCallback(
    async (event: FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      const nextToken = authInput.trim();
      if (!nextToken) {
        setAuthMessage("Enter a global API key to continue.");
        return;
      }

      setConnecting(true);
      setAuthMessage("");
      try {
        if (typeof window !== "undefined") {
          localStorage.setItem(API_KEY_STORAGE_KEY, nextToken);
        }
        setAuthToken(nextToken);
      } finally {
        setConnecting(false);
      }
    },
    [authInput]
  );

  const handleRefresh = useCallback(async () => {
    if (!authToken) {
      return;
    }

    setRefreshing(true);
    setDashboardError("");
    try {
      await loadAnalytics();
      if (offset === 0) {
        await loadRecords(0);
      } else if (latestCursor) {
        const params = buildSharedParams(0);
        params.set("limit", "1");
        params.set("since_timestamp", latestCursor.timestamp);
        params.set("since_id", String(latestCursor.id));
        const preview = await fetchWithAuth<DashboardRecordsResponse>(
          buildApiUrl("/api/dashboard/records", params)
        );
        setPendingNewRecords(preview.new_records);
      }
    } catch (error) {
      if ((error as Error).message !== "AUTH_INVALID") {
        setDashboardError("Refresh failed. Showing the most recent successful snapshot.");
      }
    } finally {
      setRefreshing(false);
    }
  }, [
    authToken,
    buildApiUrl,
    buildSharedParams,
    fetchWithAuth,
    latestCursor,
    loadAnalytics,
    loadRecords,
    offset,
  ]);

  const handleCopyRequestId = useCallback(async (requestId: string) => {
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(requestId);
      } else {
        const textarea = document.createElement("textarea");
        textarea.value = requestId;
        textarea.setAttribute("readonly", "true");
        textarea.style.position = "absolute";
        textarea.style.left = "-9999px";
        document.body.appendChild(textarea);
        textarea.select();
        document.execCommand("copy");
        textarea.remove();
      }
      setCopiedRequestId(requestId);
      window.setTimeout(() => setCopiedRequestId(null), 2000);
    } catch {
      setCopiedRequestId(requestId);
      window.setTimeout(() => setCopiedRequestId(null), 2000);
    }
  }, []);

  const handleExport = useCallback(async () => {
    if (!authToken) {
      return;
    }

    try {
      const exportPageSize = 100;
      let nextOffset = 0;
      let expectedTotal = 0;
      const exportedRecords: UsageRecord[] = [];

      do {
        const params = buildSharedParams(nextOffset);
        params.set("limit", String(exportPageSize));

        const response = await fetchWithAuth<DashboardRecordsResponse>(
          buildApiUrl("/api/dashboard/records", params)
        );

        exportedRecords.push(...response.data);
        expectedTotal = response.total;
        nextOffset += response.data.length;

        if (response.data.length === 0) {
          break;
        }
      } while (nextOffset < expectedTotal);

      downloadCsvFile(buildCsv(exportedRecords));
    } catch (error) {
      if ((error as Error).message !== "AUTH_INVALID") {
        setDashboardError("Unable to export records right now.");
      }
    }
  }, [authToken, buildApiUrl, buildSharedParams, fetchWithAuth]);

  const dashboardReady = !!authToken && !!analytics;
  const isNeutralAuthMessage = authMessage === "Disconnected";

  if (bootstrapping) {
    return <div className="min-h-screen bg-slate-100" />;
  }

  if (!authToken || !dashboardReady) {
    return (
      <main className="min-h-screen bg-[#eef2f7]">
        <header className="border-b border-[#111827] bg-white px-8 py-6">
          <div className="mx-auto flex w-full max-w-[1440px] flex-wrap items-center gap-5">
            <div className="flex items-center gap-3 pr-5 xl:border-r xl:border-[#d6dde8]">
              <Image src={apexLogo} alt="Apex Gateway logo" className="size-7 object-contain" priority />
              <span className="text-[20px] font-semibold tracking-tight text-[#17233c]">
                Apex Gateway
              </span>
            </div>
            <div className="text-[20px] font-semibold tracking-tight text-[#1f2f4d]">Dashboard</div>
          </div>
        </header>

        <section className="px-8 py-8">
          <div className="mx-auto grid w-full max-w-[1440px] gap-6 xl:grid-cols-[minmax(0,1.18fr)_minmax(420px,0.82fr)]">
            <div className="rounded-[28px] border border-[#d6dde8] bg-white p-10">
              <div className="inline-flex items-center gap-2 rounded-full border border-[#d6dde8] bg-[#eef2f7] px-3 py-1 text-xs font-semibold uppercase tracking-[0.22em] text-[#64748b]">
                <PlugZap className="size-3.5" />
                Embedded Dashboard
              </div>
              <h1 className="mt-6 text-4xl font-semibold tracking-tight text-[#17233c]">
                Apex Gateway Dashboard
              </h1>
              <p className="mt-4 max-w-2xl text-base leading-7 text-[#64748b]">
                Connect with a global API key to inspect real-time gateway throughput, routing topology,
                reliability, team usage, and request-level diagnostics.
              </p>
              <div className="mt-10 grid gap-4 sm:grid-cols-2">
                {[
                  "Overview KPIs with trend deltas",
                  "Team and model usage breakdowns",
                  "System reliability and channel latency",
                  "Request records with route provenance",
                ].map((item) => (
                  <div key={item} className="min-h-[88px] rounded-2xl border border-[#d6dde8] bg-[#f8fafc] px-5 py-5 text-[15px] leading-6 text-[#4f5f78]">
                    {item}
                  </div>
                ))}
              </div>
            </div>

            <div className="rounded-[28px] border border-[#d6dde8] bg-white p-10">
              <div className="text-sm font-semibold uppercase tracking-[0.22em] text-[#64748b]">
                Connect
              </div>
              <h2 className="mt-2 text-3xl font-semibold tracking-tight text-[#17233c]">Open the dashboard</h2>
              <p className="mt-3 text-sm leading-6 text-[#64748b]">
                URL `token` bootstrap and local storage restore are both supported. Tokens are scrubbed from the
                address bar after initialization.
              </p>

              <form className="mt-8 space-y-4" onSubmit={handleConnect}>
                <label className="block space-y-2">
                  <span className="text-sm font-medium text-[#1f2f4d]">Global API Key</span>
                  <input
                    value={authInput}
                    onChange={(event) => setAuthInput(event.target.value)}
                    placeholder="Enter API Key"
                    className="w-full rounded-2xl border border-[#d5dde8] bg-white px-4 py-3 text-sm text-[#111827] outline-none transition focus:border-[#94a3b8] focus:ring-4 focus:ring-[#e2e8f0]"
                  />
                </label>
                <Button
                  type="submit"
                  size="lg"
                  className="h-12 w-full rounded-xl bg-[#17233c] text-sm font-semibold text-white hover:bg-[#223252]"
                  disabled={connecting}
                >
                  {connecting ? (
                    <>
                      <LoaderCircle className="size-4 animate-spin" />
                      Connecting
                    </>
                  ) : (
                    "Open Dashboard"
                  )}
                </Button>
              </form>

              {authMessage ? (
                <div
                  className={[
                    "mt-4 flex items-start gap-2 rounded-2xl px-4 py-3 text-sm",
                    isNeutralAuthMessage
                      ? "border border-[#d6dde8] bg-[#f8fafc] text-[#4f5f78]"
                      : "border border-[#d9b49c] bg-[#fff1ea] text-[#83361f]",
                  ].join(" ")}
                >
                  <AlertCircle className="mt-0.5 size-4 shrink-0" />
                  <span>{authMessage}</span>
                </div>
              ) : null}

              {dashboardLoading ? (
                <div className="mt-4 flex items-center gap-2 text-sm text-[#64748b]">
                  <LoaderCircle className="size-4 animate-spin" />
                  Verifying access...
                </div>
              ) : null}
            </div>
          </div>
        </section>
      </main>
    );
  }

  return (
    <main className="min-h-screen bg-[#eef2f7]">
      <div className="min-h-screen w-full bg-[#eef2f7]">
        <header className="border-b border-[#111827] bg-white px-8 py-6">
          <div className="flex flex-col gap-4 xl:flex-row xl:items-center xl:justify-between">
            <div className="flex flex-wrap items-center gap-5">
              <div className="flex items-center gap-3 pr-5 xl:border-r xl:border-[#d6dde8]">
                <Image src={apexLogo} alt="Apex Gateway logo" className="size-7 object-contain" priority />
                <span className="text-[20px] font-semibold tracking-tight text-[#17233c]">
                  Apex Gateway
                </span>
              </div>
              <div className="text-[20px] font-semibold tracking-tight text-[#1f2f4d]">Dashboard</div>
            </div>

            <div className="flex flex-wrap items-center gap-3">
              <DashboardFilters
                range={range}
                teamId={teamId}
                model={model}
                options={filterOptions}
                onRangeChange={(value) => {
                  setRange(value);
                  setOffset(0);
                }}
                onTeamChange={(value) => {
                  setTeamId(value);
                  setOffset(0);
                }}
                onModelChange={(value) => {
                  setModel(value);
                  setOffset(0);
                }}
              />
              <Button
                variant="ghost"
                className="h-12 rounded-xl px-4 text-sm text-[#64748b] hover:bg-[#eef2f7] hover:text-[#1f2f4d]"
                onClick={() => clearAuth("Disconnected")}
              >
                Disconnect
              </Button>
            </div>
          </div>
        </header>

        <section className="space-y-6 px-8 py-8">
          {dashboardError ? (
            <div className="flex items-start gap-2 rounded-2xl border border-[#d9b49c] bg-[#fff1ea] px-4 py-3 text-sm text-[#83361f]">
              <AlertCircle className="mt-0.5 size-4 shrink-0" />
              <span>{dashboardError}</span>
            </div>
          ) : null}

          <Tabs value={activeTab} onValueChange={(value) => setActiveTab(value as DashboardTab)}>
            <TabsList className="w-fit border border-[#d6dde8] bg-[#e3e8ef]">
              <TabsTrigger value="overview" className="px-5 py-3 text-base">
                Overview
              </TabsTrigger>
              <TabsTrigger value="team" className="px-5 py-3 text-base">
                Team & Usage
              </TabsTrigger>
              <TabsTrigger value="system" className="px-5 py-3 text-base">
                System & Reliability
              </TabsTrigger>
              <TabsTrigger value="model" className="px-5 py-3 text-base">
                Model & Router
              </TabsTrigger>
              <TabsTrigger value="records" className="px-5 py-3 text-base">
                Records
              </TabsTrigger>
            </TabsList>

            <TabsContent value="overview">
              <OverviewTab analytics={analytics} />
            </TabsContent>
            <TabsContent value="team">
              <TeamUsageTab analytics={analytics} />
            </TabsContent>
            <TabsContent value="system">
              <SystemReliabilityTab analytics={analytics} />
            </TabsContent>
            <TabsContent value="model">
              <ModelRouterTab analytics={analytics} />
            </TabsContent>
            <TabsContent value="records">
              <RecordsTab
                records={records}
                total={totalRecords}
                limit={limit}
                offset={offset}
                loading={recordsLoading}
                refreshing={refreshing}
                pendingNewRecords={pendingNewRecords}
                copiedRequestId={copiedRequestId}
                onRefresh={handleRefresh}
                onJumpToLatest={() => {
                  setOffset(0);
                  setActiveTab("records");
                }}
                onPageChange={setOffset}
                onExport={handleExport}
                onCopyRequestId={handleCopyRequestId}
                onSelectRecord={setSelectedRecord}
              />
            </TabsContent>
          </Tabs>
        </section>
      </div>

      <UsageDetailsDrawer record={selectedRecord} onClose={() => setSelectedRecord(null)} />
    </main>
  );
}
