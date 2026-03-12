"use client";

import { FormEvent, KeyboardEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { AlertCircle, Clock3, Download, RefreshCw, X } from "lucide-react";
import {
  CartesianGrid,
  Line,
  LineChart,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import {
  ChartConfig,
  ChartContainer,
  ChartTooltipContent,
} from "@/components/ui/chart";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { cn } from "@/lib/utils";

interface MetricsSummary {
  total_requests: number;
  total_errors: number;
  total_fallbacks: number;
  avg_latency_ms: number;
  error_rate: number;
  p95_latency_ms: number;
}

interface TrendData {
  date: string;
  requests: number;
  input_tokens: number;
  output_tokens: number;
  total_errors: number;
  total_fallbacks: number;
  avg_latency_ms: number;
  p95_latency_ms: number;
}

interface TrendResponse {
  period: string;
  data: TrendData[];
}

interface UsageRecord {
  id: number;
  timestamp: string;
  request_id?: string | null;
  team_id: string;
  router: string;
  channel: string;
  model: string;
  input_tokens: number;
  output_tokens: number;
  latency_ms?: number | null;
  fallback_triggered: boolean;
  status: string;
  status_code?: number | null;
  error_message?: string | null;
  provider_trace_id?: string | null;
  provider_error_body?: string | null;
}

interface UsageResponse {
  data: UsageRecord[];
  total: number;
  limit: number;
  offset: number;
}

type TimeRange = "today" | "week" | "month" | "custom";
type StatusFilter = "" | "errors" | "error" | "fallback_error" | "fallbacks" | "fallback";
type TrendDotProps = {
  cx?: number;
  cy?: number;
  payload?: TrendData;
  stroke?: string;
};
type DrilldownState = {
  source: "kpi" | "trend";
  label: string;
  focusDate?: string;
  status?: StatusFilter;
} | null;

const API_KEY_STORAGE_KEY = "apex-api-key";
const DASHBOARD_PATH = "/dashboard/";
const STATUS_FILTER_LABELS: Record<Exclude<StatusFilter, "">, string> = {
  errors: "Errors",
  error: "Error",
  fallback_error: "Fallback Errors",
  fallbacks: "Fallbacks",
  fallback: "Fallback",
};

function normalizeDashboardPath(pathname: string | null): string {
  if (!pathname || pathname === "/dashboard" || pathname === "/dashboard/index.html") {
    return DASHBOARD_PATH;
  }

  return pathname;
}

function getDefaultStartDate() {
  const now = new Date();
  return new Date(now.getTime() - 7 * 24 * 60 * 60 * 1000).toISOString().split("T")[0];
}

function getToday() {
  return new Date().toISOString().split("T")[0];
}

function getStatusFilterLabel(status: StatusFilter) {
  if (!status) {
    return "";
  }

  return STATUS_FILTER_LABELS[status] ?? status;
}

export default function DashboardClient() {
  const router = useRouter();
  const pathname = usePathname();
  const searchParams = useSearchParams();

  const [bootstrapping, setBootstrapping] = useState(true);
  const [authLoading, setAuthLoading] = useState(false);
  const [dashboardLoading, setDashboardLoading] = useState(false);
  const [dashboardReady, setDashboardReady] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [metrics, setMetrics] = useState<MetricsSummary | null>(null);
  const [trends, setTrends] = useState<TrendData[]>([]);
  const [usage, setUsage] = useState<UsageRecord[]>([]);
  const [timeRange, setTimeRange] = useState<TimeRange>("week");
  const [customStartDate, setCustomStartDate] = useState("");
  const [customEndDate, setCustomEndDate] = useState("");
  const [autoRefresh, setAutoRefresh] = useState(true);
  const [filters, setFilters] = useState({
    team_id: "",
    router: "",
    channel: "",
    model: "",
    status: "" as StatusFilter,
  });
  const [pagination, setPagination] = useState({
    limit: 20,
    offset: 0,
  });
  const [total, setTotal] = useState(0);
  const [tableLoading, setTableLoading] = useState(false);
  const [authToken, setAuthToken] = useState("");
  const [authInput, setAuthInput] = useState("");
  const [authMessage, setAuthMessage] = useState("");
  const [requestError, setRequestError] = useState("");
  const [exportError, setExportError] = useState("");
  const [lastUpdated, setLastUpdated] = useState<string | null>(null);
  const [selectedRecord, setSelectedRecord] = useState<UsageRecord | null>(null);
  const [drilldown, setDrilldown] = useState<DrilldownState>(null);
  const [exporting, setExporting] = useState(false);
  const usageEffectInitialized = useRef(false);
  const trendsEffectInitialized = useRef(false);

  const apiBaseUrl = useMemo(() => {
    if (typeof window === "undefined") {
      return process.env.NEXT_PUBLIC_API_URL ?? "";
    }

    return process.env.NEXT_PUBLIC_API_URL ?? window.location.origin;
  }, []);

  const getApiUrl = useCallback((path: string) => `${apiBaseUrl}${path}`, [apiBaseUrl]);

  const formatNumber = (num: number) => new Intl.NumberFormat().format(num);
  const formatPercent = (num: number) => `${num.toFixed(1)}%`;
  const formatLatency = (ms: number) => ms.toFixed(1);

  const reliabilityTrends = useMemo(
    () =>
      trends.map((trend) => ({
        ...trend,
        error_rate: trend.requests > 0 ? (trend.total_errors / trend.requests) * 100 : 0,
      })),
    [trends]
  );

  const headerSummary = useMemo(() => {
    switch (timeRange) {
      case "today":
        return "Showing the latest 24-hour gateway activity.";
      case "week":
        return "Showing the latest 7-day gateway activity.";
      case "month":
        return "Showing the latest 30-day gateway activity.";
      case "custom":
        return "Showing a custom dashboard time range.";
      default:
        return "Showing gateway activity.";
    }
  }, [timeRange]);

  const activeFilters = useMemo(
    () =>
      [
        filters.team_id ? { key: "team_id", label: `Team: ${filters.team_id}` } : null,
        filters.router ? { key: "router", label: `Router: ${filters.router}` } : null,
        filters.channel ? { key: "channel", label: `Channel: ${filters.channel}` } : null,
        filters.model ? { key: "model", label: `Model: ${filters.model}` } : null,
        filters.status
          ? {
              key: "status",
              label: `Status: ${getStatusFilterLabel(filters.status)}`,
            }
          : null,
      ].filter(Boolean) as Array<{ key: keyof typeof filters; label: string }>,
    [filters]
  );

  const drilldownSummary = useMemo(() => {
    if (!drilldown) {
      return "";
    }

    const details = [
      drilldown.focusDate ? `Date ${drilldown.focusDate}` : "",
      drilldown.status ? `Status ${getStatusFilterLabel(drilldown.status)}` : "",
    ].filter(Boolean);

    return details.length > 0 ? `${drilldown.label} · ${details.join(" · ")}` : drilldown.label;
  }, [drilldown]);

  const chartConfig: ChartConfig = {
    requests: { label: "Requests", color: "hsl(214, 84%, 56%)" },
    input_tokens: { label: "Input Tokens", color: "hsl(160, 84%, 39%)" },
    output_tokens: { label: "Output Tokens", color: "hsl(221, 83%, 53%)" },
    error_rate: { label: "Error Rate", color: "hsl(352, 83%, 63%)" },
    p95_latency_ms: { label: "P95 Latency", color: "hsl(43, 96%, 56%)" },
  };

  const syncUrlState = useCallback(
    (
      nextTimeRange: TimeRange,
      nextStartDate: string,
      nextEndDate: string,
      nextFilters: typeof filters,
      nextOffset: number
    ) => {
      const params = new URLSearchParams();

      params.set("range", nextTimeRange);
      if (nextTimeRange === "custom") {
        if (nextStartDate) params.set("start_date", nextStartDate);
        if (nextEndDate) params.set("end_date", nextEndDate);
      }

      if (nextFilters.team_id) params.set("team_id", nextFilters.team_id);
      if (nextFilters.router) params.set("router", nextFilters.router);
      if (nextFilters.channel) params.set("channel", nextFilters.channel);
      if (nextFilters.model) params.set("model", nextFilters.model);
      if (nextFilters.status) params.set("status", nextFilters.status);
      if (nextOffset > 0) params.set("offset", String(nextOffset));

      const query = params.toString();
      const nextPath = query ? `${normalizeDashboardPath(pathname)}?${query}` : normalizeDashboardPath(pathname);

      if (typeof window !== "undefined") {
        window.history.replaceState({}, "", nextPath);
        return;
      }

      router.replace(nextPath, { scroll: false });
    },
    [pathname, router]
  );

  const clearAuth = useCallback((message = "") => {
    localStorage.removeItem(API_KEY_STORAGE_KEY);
    setAuthToken("");
    setAuthInput("");
    setBootstrapping(false);
    setAuthLoading(false);
    setDashboardLoading(false);
    setDashboardReady(false);
    setRefreshing(false);
    setTableLoading(false);
    setMetrics(null);
    setTrends([]);
    setUsage([]);
    setTotal(0);
    setRequestError("");
    setExportError("");
    setAuthMessage(message);
    setLastUpdated(null);
    setSelectedRecord(null);
    setDrilldown(null);
    setExporting(false);
    usageEffectInitialized.current = false;
    trendsEffectInitialized.current = false;
  }, []);

  useEffect(() => {
    const queryToken = searchParams.get("auth_token")?.trim() ?? "";
    const storedToken = localStorage.getItem(API_KEY_STORAGE_KEY)?.trim() ?? "";
    const token = queryToken || storedToken;

    const rangeParam = searchParams.get("range");
    const startParam = searchParams.get("start_date") ?? "";
    const endParam = searchParams.get("end_date") ?? "";
    const offsetParam = Number(searchParams.get("offset") ?? "0");

    if (rangeParam === "today" || rangeParam === "week" || rangeParam === "month" || rangeParam === "custom") {
      setTimeRange(rangeParam);
    }
    setCustomStartDate(startParam);
    setCustomEndDate(endParam);
    setFilters({
      team_id: searchParams.get("team_id") ?? "",
      router: searchParams.get("router") ?? "",
      channel: searchParams.get("channel") ?? "",
      model: searchParams.get("model") ?? "",
      status: (searchParams.get("status") as StatusFilter | null) ?? "",
    });
    setPagination((current) => ({
      ...current,
      offset: Number.isNaN(offsetParam) ? 0 : Math.max(0, offsetParam),
    }));
    if (rangeParam === "custom") {
      setAutoRefresh(false);
    }

    if (queryToken) {
      localStorage.setItem(API_KEY_STORAGE_KEY, queryToken);

      if (typeof window !== "undefined") {
        const params = new URLSearchParams(searchParams.toString());
        params.delete("auth_token");
        const query = params.toString();
        const nextPath = query ? `${normalizeDashboardPath(pathname)}?${query}` : normalizeDashboardPath(pathname);
        window.history.replaceState({}, "", nextPath);
      }
    }

    if (token) {
      setDashboardLoading(true);
      setDashboardReady(false);
    }

    setAuthInput(token);
    setAuthToken(token);
    setBootstrapping(false);
  }, [pathname, router, searchParams]);

  const getDateRange = useCallback(() => {
    const today = getToday();
    const weekAgo = getDefaultStartDate();

    if (timeRange === "today") {
      return { start_date: today, end_date: today };
    }

    if (timeRange === "week") {
      return { start_date: weekAgo, end_date: today };
    }

    if (timeRange === "month") {
      const monthAgo = new Date(Date.now() - 30 * 24 * 60 * 60 * 1000).toISOString().split("T")[0];
      return { start_date: monthAgo, end_date: today };
    }

    return {
      start_date: customStartDate || weekAgo,
      end_date: customEndDate || today,
    };
  }, [customEndDate, customStartDate, timeRange]);

  const buildAuthHeaders = useCallback(() => ({ Authorization: `Bearer ${authToken}` }), [authToken]);

  const buildUsageParams = useCallback(
    (limit: number, offset: number) => {
      const { start_date, end_date } = getDateRange();
      const params = new URLSearchParams();
      if (filters.team_id) params.set("team_id", filters.team_id);
      if (filters.router) params.set("router", filters.router);
      if (filters.channel) params.set("channel", filters.channel);
      if (filters.model) params.set("model", filters.model);
      if (filters.status) params.set("status", filters.status);
      params.set("start_date", start_date);
      params.set("end_date", end_date);
      params.set("limit", String(limit));
      params.set("offset", String(offset));
      return params;
    },
    [filters, getDateRange]
  );

  const csvEscape = (value: string | number | boolean | null | undefined) => {
    const normalized = value == null ? "" : String(value);
    if (/[",\n]/.test(normalized)) {
      return `"${normalized.replace(/"/g, '""')}"`;
    }

    return normalized;
  };

  const buildUsageCsv = useCallback(
    (records: UsageRecord[]) => {
      const headers = [
        "timestamp",
        "request_id",
        "team_id",
        "router",
        "channel",
        "model",
        "input_tokens",
        "output_tokens",
        "total_tokens",
        "latency_ms",
        "status",
        "fallback_triggered",
        "status_code",
        "error_message",
        "provider_trace_id",
        "provider_error_body",
      ];

      const rows = records.map((record) =>
        [
          record.timestamp,
          record.request_id ?? "",
          record.team_id,
          record.router,
          record.channel,
          record.model,
          record.input_tokens,
          record.output_tokens,
          record.input_tokens + record.output_tokens,
          record.latency_ms ?? "",
          record.status,
          record.fallback_triggered,
          record.status_code ?? "",
          record.error_message ?? "",
          record.provider_trace_id ?? "",
          record.provider_error_body ?? "",
        ]
          .map(csvEscape)
          .join(",")
      );

      return [headers.join(","), ...rows].join("\n");
    },
    []
  );

  const downloadCsv = useCallback((content: string) => {
    const blob = new Blob([content], { type: "text/csv;charset=utf-8;" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `apex-usage-report-${new Date().toISOString().replace(/[:.]/g, "-")}.csv`;
    document.body.appendChild(anchor);
    anchor.click();
    anchor.remove();
    URL.revokeObjectURL(url);
  }, []);

  const fetchMetrics = useCallback(async () => {
    const res = await fetch(getApiUrl("/api/metrics"), {
      headers: buildAuthHeaders(),
    });

    if (res.status === 401 || res.status === 403) {
      clearAuth("Invalid API Key");
      throw new Error("AUTH_INVALID");
    }

    if (!res.ok) {
      throw new Error(`Metrics request failed with status ${res.status}`);
    }

    const data = await res.json();
    setMetrics(data);
    return true;
  }, [buildAuthHeaders, clearAuth, getApiUrl]);

  const fetchTrends = useCallback(async () => {
    const { start_date, end_date } = getDateRange();
    const params = new URLSearchParams({
      period: "daily",
      start_date,
      end_date,
    });

    const res = await fetch(getApiUrl(`/api/metrics/trends?${params}`), {
      headers: buildAuthHeaders(),
    });

    if (res.status === 401 || res.status === 403) {
      clearAuth("Invalid API Key");
      throw new Error("AUTH_INVALID");
    }

    if (!res.ok) {
      throw new Error(`Trends request failed with status ${res.status}`);
    }

    const data: TrendResponse = await res.json();
    setTrends(data.data);
  }, [buildAuthHeaders, clearAuth, getApiUrl, getDateRange]);

  const fetchUsage = useCallback(async (options?: { background?: boolean }) => {
    const background = options?.background ?? false;
    if (!background) {
      setTableLoading(true);
    }

    try {
      const params = buildUsageParams(pagination.limit, pagination.offset);

      const res = await fetch(getApiUrl(`/api/usage?${params}`), {
        headers: buildAuthHeaders(),
      });

      if (res.status === 401 || res.status === 403) {
        clearAuth("Invalid API Key");
        throw new Error("AUTH_INVALID");
      }

      if (!res.ok) {
        throw new Error(`Usage request failed with status ${res.status}`);
      }

      const data: UsageResponse = await res.json();
      setUsage(data.data);
      setTotal(data.total);
      if (data.data.length === 0) {
        setSelectedRecord(null);
      }
    } finally {
      if (!background) {
        setTableLoading(false);
      }
    }
  }, [buildAuthHeaders, buildUsageParams, clearAuth, getApiUrl, pagination.limit, pagination.offset]);

  const loadDashboardData = useCallback(async (options?: { initial?: boolean }) => {
    const initial = options?.initial ?? false;
    let authInvalid = false;

    if (initial) {
      setDashboardLoading(true);
      setDashboardReady(false);
    } else {
      setRefreshing(true);
    }

    setAuthMessage("");
    setRequestError("");

    try {
      const metricsOk = await fetchMetrics();
      if (!metricsOk) {
        return;
      }

      await Promise.all([fetchTrends(), fetchUsage({ background: true })]);
      setLastUpdated(new Date().toLocaleTimeString());
    } catch (error) {
      if (error instanceof Error && error.message === "AUTH_INVALID") {
        authInvalid = true;
        return;
      }

      setRequestError("Unable to fetch the latest dashboard data right now.");
    } finally {
      if (authInvalid) {
        return;
      }

      if (initial) {
        setDashboardLoading(false);
        setDashboardReady(true);
      } else {
        setRefreshing(false);
      }
    }
  }, [fetchMetrics, fetchTrends, fetchUsage]);

  const validateAndSetToken = useCallback(
    async (token: string) => {
      const trimmedToken = token.trim();
      if (!trimmedToken) {
        setAuthMessage("API Key is required");
        return;
      }

      setAuthLoading(true);
      setAuthMessage("");
      setRequestError("");

      try {
        const res = await fetch(getApiUrl("/api/metrics"), {
          headers: { Authorization: `Bearer ${trimmedToken}` },
        });

        if (res.status === 401 || res.status === 403) {
          setAuthMessage("Invalid API Key");
          return;
        }

        if (!res.ok) {
          setRequestError("Dashboard API is unavailable right now.");
          return;
        }

        localStorage.setItem(API_KEY_STORAGE_KEY, trimmedToken);
        setDashboardLoading(true);
        setDashboardReady(false);
        setAuthInput(trimmedToken);
        setAuthToken(trimmedToken);
      } catch {
        setRequestError("Dashboard API is unavailable right now.");
      } finally {
        setAuthLoading(false);
      }
    },
    [getApiUrl]
  );

  const handleAuthenticate = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    await validateAndSetToken(authInput);
  };

  const handleExportCsv = useCallback(async () => {
    let authInvalid = false;
    setExporting(true);
    setExportError("");

    try {
      const exportLimit = 100;
      let nextOffset = 0;
      let totalRecords = 0;
      const allRecords: UsageRecord[] = [];

      do {
        const params = buildUsageParams(exportLimit, nextOffset);
        const res = await fetch(getApiUrl(`/api/usage?${params}`), {
          headers: buildAuthHeaders(),
        });

        if (res.status === 401 || res.status === 403) {
          clearAuth("Invalid API Key");
          authInvalid = true;
          return;
        }

        if (!res.ok) {
          throw new Error(`Export request failed with status ${res.status}`);
        }

        const data: UsageResponse = await res.json();
        allRecords.push(...data.data);
        totalRecords = data.total;
        nextOffset += data.data.length;

        if (data.data.length === 0) {
          break;
        }
      } while (nextOffset < totalRecords);

      downloadCsv(buildUsageCsv(allRecords));
    } catch {
      setExportError("Unable to export the current filtered usage records.");
    } finally {
      if (!authInvalid) {
        setExporting(false);
      }
    }
  }, [buildAuthHeaders, buildUsageCsv, buildUsageParams, clearAuth, downloadCsv, getApiUrl]);

  const updateFilter = (key: keyof typeof filters, value: string) => {
    const nextFilters = { ...filters, [key]: value as StatusFilter };
    const nextOffset = 0;
    setFilters(nextFilters);
    setSelectedRecord(null);
    setDrilldown(null);
    setPagination((current) => ({ ...current, offset: nextOffset }));
    syncUrlState(timeRange, customStartDate, customEndDate, nextFilters, nextOffset);
  };

  const removeFilterChip = (key: keyof typeof filters) => {
    updateFilter(key, "");
  };

  const resetFilters = () => {
    const nextFilters = {
      team_id: "",
      router: "",
      channel: "",
      model: "",
      status: "" as StatusFilter,
    };
    setFilters(nextFilters);
    setSelectedRecord(null);
    setDrilldown(null);
    setPagination((current) => ({ ...current, offset: 0 }));
    syncUrlState(timeRange, customStartDate, customEndDate, nextFilters, 0);
  };

  const changeTimeRange = (range: TimeRange) => {
    setTimeRange(range);
    const shouldEnableAutoRefresh = range !== "custom";
    setAutoRefresh(shouldEnableAutoRefresh);
    setPagination((current) => ({ ...current, offset: 0 }));
    syncUrlState(range, customStartDate, customEndDate, filters, 0);
  };

  const updateCustomDate = (kind: "start" | "end", value: string) => {
    const nextStartDate = kind === "start" ? value : customStartDate;
    const nextEndDate = kind === "end" ? value : customEndDate;
    if (kind === "start") setCustomStartDate(value);
    if (kind === "end") setCustomEndDate(value);
    setDrilldown(null);
    setPagination((current) => ({ ...current, offset: 0 }));
    syncUrlState(timeRange, nextStartDate, nextEndDate, filters, 0);
  };

  const focusUsageRecords = useCallback(() => {
    if (typeof document === "undefined") {
      return;
    }

    document.getElementById("usage-records-table")?.scrollIntoView({ behavior: "smooth", block: "start" });
  }, []);

  const applyUsageDrilldown = useCallback(
    ({
      nextStatus,
      focusDate,
      label,
    }: {
      nextStatus?: StatusFilter;
      focusDate?: string;
      label: string;
    }) => {
      const nextFilters = {
        ...filters,
        status: nextStatus ?? filters.status,
      };
      const nextRange = focusDate ? "custom" : timeRange;
      const nextStartDate = focusDate ?? customStartDate;
      const nextEndDate = focusDate ?? customEndDate;

      setFilters(nextFilters);
      setSelectedRecord(null);
      setDrilldown({
        source: focusDate ? "trend" : "kpi",
        label,
        focusDate,
        status: nextStatus,
      });
      setPagination((current) => ({ ...current, offset: 0 }));

      if (focusDate) {
        setTimeRange("custom");
        setCustomStartDate(focusDate);
        setCustomEndDate(focusDate);
        setAutoRefresh(false);
      }

      syncUrlState(nextRange, nextStartDate, nextEndDate, nextFilters, 0);
      window.setTimeout(() => focusUsageRecords(), 50);
    },
    [customEndDate, customStartDate, filters, focusUsageRecords, syncUrlState, timeRange]
  );

  const renderTrendDot = useCallback(
    (seriesLabel: string, nextStatus?: StatusFilter) =>
      function TrendDot({ cx, cy, payload, stroke }: TrendDotProps) {
        if (typeof cx !== "number" || typeof cy !== "number" || !payload?.date) {
          return <circle cx={0} cy={0} r={0} fill="transparent" stroke="none" />;
        }

        const handleActivate = () =>
          applyUsageDrilldown({
            nextStatus,
            focusDate: payload.date,
            label: `${seriesLabel} spike`,
          });
        const handleKeyDown = (event: KeyboardEvent<SVGCircleElement>) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            handleActivate();
          }
        };

        return (
          <circle
            cx={cx}
            cy={cy}
            r={5}
            fill={stroke ?? "#38bdf8"}
            stroke="rgba(2, 6, 23, 0.9)"
            strokeWidth={2}
            role="button"
            tabIndex={0}
            aria-label={`Inspect ${seriesLabel} records for ${payload.date}`}
            className="cursor-pointer outline-none"
            onClick={handleActivate}
            onKeyDown={handleKeyDown}
          />
        );
      },
    [applyUsageDrilldown]
  );

  const goToPage = (direction: "prev" | "next") => {
    const nextOffset =
      direction === "prev"
        ? Math.max(0, pagination.offset - pagination.limit)
        : pagination.offset + pagination.limit;
    setPagination((current) => ({ ...current, offset: nextOffset }));
    syncUrlState(timeRange, customStartDate, customEndDate, filters, nextOffset);
  };

  const handleDisconnect = () => {
    clearAuth("");
  };

  useEffect(() => {
    if (!authToken) {
      return;
    }

    usageEffectInitialized.current = false;
    trendsEffectInitialized.current = false;
    void loadDashboardData({ initial: true });
  }, [authToken, loadDashboardData]);

  useEffect(() => {
    if (!authToken || !dashboardReady) {
      return;
    }

    if (!usageEffectInitialized.current) {
      usageEffectInitialized.current = true;
      return;
    }

    void fetchUsage();
  }, [authToken, dashboardReady, fetchUsage]);

  useEffect(() => {
    if (!authToken || !dashboardReady) {
      return;
    }

    if (!trendsEffectInitialized.current) {
      trendsEffectInitialized.current = true;
      return;
    }

    void fetchTrends();
  }, [authToken, dashboardReady, fetchTrends]);

  useEffect(() => {
    if (!authToken || !autoRefresh || timeRange === "custom") {
      return;
    }

    const timer = window.setInterval(() => {
      void loadDashboardData();
    }, 30000);

    return () => window.clearInterval(timer);
  }, [authToken, autoRefresh, loadDashboardData, timeRange]);

  if (!authToken) {
    return (
      <div className="min-h-screen bg-[radial-gradient(circle_at_top,_rgba(59,130,246,0.08),_transparent_35%),linear-gradient(180deg,#f8fafc_0%,#eef2ff_100%)] px-4 py-12">
        <div className="mx-auto flex min-h-[80vh] max-w-md items-center">
          <div className="w-full space-y-4">
            <div className="space-y-2 text-center">
              <p className="text-sm font-medium uppercase tracking-[0.18em] text-sky-700">
                Apex Gateway
              </p>
              <h1 className="text-4xl font-semibold tracking-tight text-slate-950">
                Apex Gateway Dashboard
              </h1>
              <p className="text-sm text-slate-600">
                Use an API Key to access live gateway health, trends, and usage records.
              </p>
            </div>
            <Card className="border border-slate-200/80 bg-white/80 shadow-lg shadow-slate-200/50 backdrop-blur">
              <CardContent className="pt-6">
                <form onSubmit={handleAuthenticate} className="space-y-4">
                  <Input
                    type="password"
                    placeholder="Enter API Key"
                    value={authInput}
                    onChange={(event) => setAuthInput(event.target.value)}
                  />
                  {authMessage ? (
                    <p className="text-sm text-red-600">{authMessage}</p>
                  ) : null}
                  {requestError ? (
                    <p className="text-sm text-amber-700">{requestError}</p>
                  ) : null}
                  <Button type="submit" className="w-full" disabled={bootstrapping || authLoading}>
                    {authLoading ? "Connecting..." : "Open Dashboard"}
                  </Button>
                </form>
              </CardContent>
            </Card>
          </div>
        </div>
      </div>
    );
  }

  if (dashboardLoading) {
    return (
      <div className="min-h-screen bg-slate-950 px-4 py-8 text-slate-50">
        <div className="mx-auto max-w-7xl space-y-6">
          <div className="grid gap-4 lg:grid-cols-4">
            {Array.from({ length: 4 }).map((_, index) => (
              <Card key={index} className="border border-white/10 bg-white/5">
                <CardContent className="space-y-3 py-8">
                  <div className="h-3 w-24 animate-pulse rounded bg-white/10" />
                  <div className="h-8 w-32 animate-pulse rounded bg-white/10" />
                </CardContent>
              </Card>
            ))}
          </div>
          <Card className="border border-white/10 bg-white/5">
            <CardContent className="py-10 text-center text-sm text-slate-300">
              Loading dashboard data...
            </CardContent>
          </Card>
        </div>
      </div>
    );
  }

  const showingStart = total === 0 ? 0 : pagination.offset + 1;
  const showingEnd = total === 0 ? 0 : pagination.offset + usage.length;

  return (
    <div className="min-h-screen bg-[linear-gradient(180deg,#020617_0%,#0f172a_38%,#111827_100%)] px-4 py-6 text-slate-50 sm:px-6 lg:px-8">
      <div className="mx-auto max-w-7xl space-y-6">
        <section className="rounded-3xl border border-white/10 bg-white/[0.03] p-5 shadow-2xl shadow-slate-950/50 backdrop-blur">
          <div className="flex flex-col gap-5 xl:flex-row xl:items-start xl:justify-between">
            <div className="space-y-3">
              <div className="space-y-2">
                <p className="text-xs font-medium uppercase tracking-[0.24em] text-sky-300/80">
                  Monitor
                </p>
                <div>
                  <h1 className="text-3xl font-semibold tracking-tight text-white">
                    Apex Gateway Dashboard
                  </h1>
                  <p className="mt-1 text-sm text-slate-300">{headerSummary}</p>
                </div>
              </div>
              <div className="flex flex-wrap items-center gap-3 text-sm text-slate-300">
                <span className="inline-flex items-center gap-2 rounded-full border border-emerald-400/30 bg-emerald-400/10 px-3 py-1 text-emerald-200">
                  <span className="size-2 rounded-full bg-emerald-400" />
                  Connected
                </span>
                <span className="inline-flex items-center gap-2">
                  <Clock3 className="size-4 text-slate-400" />
                  {refreshing
                    ? "Refreshing dashboard data..."
                    : lastUpdated
                      ? `Last updated ${lastUpdated}`
                      : "Waiting for first successful sync"}
                </span>
              </div>
            </div>

            <div className="flex flex-wrap items-center gap-2">
              <Button
                variant="outline"
                onClick={() => void loadDashboardData()}
                disabled={refreshing}
                className="border-white/15 bg-white/5 text-slate-100 hover:bg-white/10"
              >
                <RefreshCw className={cn("size-4", refreshing && "animate-spin")} />
                {refreshing ? "Refreshing..." : "Refresh"}
              </Button>
              <Button variant="outline" onClick={handleDisconnect} className="border-white/15 bg-white/5 text-slate-100 hover:bg-white/10">
                Disconnect
              </Button>
            </div>
          </div>
        </section>

        <section className="rounded-3xl border border-white/10 bg-slate-900/70 p-4 shadow-lg shadow-slate-950/30">
          <div className="flex flex-col gap-4 xl:flex-row xl:items-center xl:justify-between">
            <div className="flex flex-wrap items-center gap-2">
              {(["today", "week", "month", "custom"] as TimeRange[]).map((range) => (
                <Button
                  key={range}
                  variant={timeRange === range ? "default" : "outline"}
                  size="sm"
                  onClick={() => changeTimeRange(range)}
                  className={cn(
                    timeRange === range
                      ? "bg-sky-500 text-slate-950 hover:bg-sky-400"
                      : "border-white/10 bg-white/5 text-slate-100 hover:bg-white/10"
                  )}
                >
                  {range === "today"
                    ? "24H"
                    : range === "week"
                      ? "This Week"
                      : range === "month"
                        ? "30D"
                        : "Custom"}
                </Button>
              ))}
            </div>

            <div className="flex flex-wrap items-center gap-3 text-sm text-slate-200">
              <button
                type="button"
                onClick={() => setAutoRefresh((current) => !current)}
                className={cn(
                  "inline-flex items-center gap-2 rounded-full border px-3 py-1.5 transition-colors",
                  autoRefresh
                    ? "border-emerald-400/30 bg-emerald-400/10 text-emerald-100"
                    : "border-white/10 bg-white/5 text-slate-300"
                )}
              >
                <span
                  className={cn(
                    "size-2 rounded-full",
                    autoRefresh ? "bg-emerald-400" : "bg-slate-500"
                  )}
                />
                Auto refresh {autoRefresh ? "On" : "Off"}
              </button>
              <Button
                variant="outline"
                onClick={() => void handleExportCsv()}
                disabled={exporting}
                className="border-white/10 bg-white/5 text-slate-100 hover:bg-white/10"
              >
                <Download className="size-4" />
                {exporting ? "Exporting..." : "Export CSV"}
              </Button>
            </div>
          </div>

          {timeRange === "custom" ? (
            <div className="mt-4 flex flex-wrap items-center gap-3">
              <Input
                type="date"
                value={customStartDate}
                onChange={(event) => updateCustomDate("start", event.target.value)}
                className="w-full border-white/10 bg-white/5 text-slate-50 sm:w-44"
              />
              <span className="text-sm text-slate-400">to</span>
              <Input
                type="date"
                value={customEndDate}
                onChange={(event) => updateCustomDate("end", event.target.value)}
                className="w-full border-white/10 bg-white/5 text-slate-50 sm:w-44"
              />
            </div>
          ) : null}
        </section>

        {exportError ? (
          <section className="rounded-2xl border border-amber-400/20 bg-amber-400/8 px-4 py-3 text-sm text-amber-100/90">
            {exportError}
          </section>
        ) : null}

        {requestError ? (
          <section className="rounded-2xl border border-amber-400/30 bg-amber-400/10 px-4 py-3 text-amber-50">
            <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
              <div className="flex items-start gap-3">
                <AlertCircle className="mt-0.5 size-4 shrink-0" />
                <div>
                  <p className="font-medium">Dashboard sync issue</p>
                  <p className="text-sm text-amber-100/90">{requestError}</p>
                </div>
              </div>
              <Button variant="outline" onClick={() => void loadDashboardData()} className="border-amber-200/25 bg-transparent text-amber-50 hover:bg-amber-200/10">
                Retry
              </Button>
            </div>
          </section>
        ) : null}

        <section className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
          <MetricCard
            label="Total Requests"
            value={metrics ? formatNumber(metrics.total_requests) : "-"}
            tone="neutral"
            hint="Gateway traffic across the selected time range."
            actionLabel="Clear incident filters and inspect the current table."
            onClick={() => applyUsageDrilldown({ nextStatus: "", label: "Total Requests" })}
          />
          <MetricCard
            label="Error Rate"
            value={metrics ? formatPercent(metrics.error_rate) : "-"}
            tone={
              metrics && metrics.error_rate >= 5
                ? "critical"
                : metrics && metrics.error_rate >= 2
                  ? "warning"
                  : "healthy"
            }
            hint="Derived from total errors divided by total requests."
            actionLabel="Filter usage records down to error windows."
            onClick={() => applyUsageDrilldown({ nextStatus: "errors", label: "Error Rate" })}
          />
          <MetricCard
            label="P95 Latency"
            value={metrics ? `${formatLatency(metrics.p95_latency_ms || metrics.avg_latency_ms)} ms` : "-"}
            tone={metrics && (metrics.p95_latency_ms || metrics.avg_latency_ms) >= 600 ? "warning" : "neutral"}
            hint="P95 latency highlights slow-tail behavior better than averages."
            actionLabel="Jump to the slowest day in the current range."
            onClick={() => {
              const slowestTrend = reliabilityTrends.reduce<TrendData | null>((slowest, trend) => {
                if (!slowest || trend.p95_latency_ms > slowest.p95_latency_ms) {
                  return trend;
                }

                return slowest;
              }, null);

              applyUsageDrilldown({ focusDate: slowestTrend?.date, label: "P95 Latency" });
            }}
          />
          <MetricCard
            label="Fallbacks"
            value={metrics ? formatNumber(metrics.total_fallbacks) : "-"}
            tone={metrics && metrics.total_fallbacks > 0 ? "warning" : "healthy"}
            hint="Fallbacks show how often the primary route was not enough."
            actionLabel="Filter usage records to fallback traffic."
            onClick={() => applyUsageDrilldown({ nextStatus: "fallbacks", label: "Fallbacks" })}
          />
        </section>

        {drilldown ? (
          <section className="rounded-2xl border border-sky-400/25 bg-sky-400/10 px-4 py-3 text-sky-50">
            <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
              <div className="space-y-1">
                <p className="text-xs font-medium uppercase tracking-[0.2em] text-sky-200/80">
                  Drilldown active
                </p>
                <p className="text-sm font-medium">{drilldownSummary}</p>
                <p className="text-xs text-sky-100/80">
                  Usage Records is currently focused by {drilldown.source === "trend" ? "trend point" : "KPI card"}.
                </p>
              </div>
              <Button
                type="button"
                variant="outline"
                onClick={resetFilters}
                className="border-sky-200/30 bg-transparent text-sky-50 hover:bg-sky-200/10"
              >
                Clear drilldown
              </Button>
            </div>
          </section>
        ) : null}

        <section className="grid gap-4 xl:grid-cols-[1.3fr_1fr]">
          <Card className="border border-white/10 bg-white/[0.04]">
            <CardHeader>
              <CardTitle className="text-white">Request Volume Trend</CardTitle>
              <CardDescription className="text-slate-400">
                Track request changes over the selected time window.
              </CardDescription>
            </CardHeader>
            <CardContent>
              <ChartContainer config={chartConfig} className="h-72">
                <LineChart data={trends}>
                  <CartesianGrid stroke="rgba(148, 163, 184, 0.18)" strokeDasharray="3 3" />
                  <XAxis dataKey="date" tick={{ fill: "#cbd5e1", fontSize: 12 }} />
                  <YAxis tick={{ fill: "#cbd5e1", fontSize: 12 }} />
                  <Tooltip content={<ChartTooltipContent />} />
                  <Line
                    type="monotone"
                    dataKey="requests"
                    stroke="hsl(214, 84%, 56%)"
                    strokeWidth={2.5}
                    dot={renderTrendDot("request volume")}
                    activeDot={{ r: 6 }}
                  />
                </LineChart>
              </ChartContainer>
              <p className="mt-3 text-xs text-slate-500">
                Click a point to focus the usage table on that exact day.
              </p>
            </CardContent>
          </Card>

          <Card className="border border-white/10 bg-white/[0.04]">
            <CardHeader>
              <CardTitle className="text-white">Reliability Trend</CardTitle>
              <CardDescription className="text-slate-400">
                Error rate and P95 latency move together to reveal unstable windows.
              </CardDescription>
            </CardHeader>
            <CardContent>
              <ChartContainer config={chartConfig} className="h-72">
                <LineChart data={reliabilityTrends}>
                  <CartesianGrid stroke="rgba(148, 163, 184, 0.18)" strokeDasharray="3 3" />
                  <XAxis dataKey="date" tick={{ fill: "#cbd5e1", fontSize: 12 }} />
                  <YAxis
                    yAxisId="left"
                    tick={{ fill: "#cbd5e1", fontSize: 12 }}
                    tickFormatter={(value) => `${value}%`}
                  />
                  <YAxis
                    yAxisId="right"
                    orientation="right"
                    tick={{ fill: "#cbd5e1", fontSize: 12 }}
                    tickFormatter={(value) => `${value}ms`}
                  />
                  <Tooltip content={<ChartTooltipContent />} />
                  <Line
                    yAxisId="left"
                    type="monotone"
                    dataKey="error_rate"
                    stroke="hsl(352, 83%, 63%)"
                    strokeWidth={2.5}
                    dot={renderTrendDot("error", "errors")}
                    activeDot={{ r: 6 }}
                    name="Error Rate"
                  />
                  <Line
                    yAxisId="right"
                    type="monotone"
                    dataKey="p95_latency_ms"
                    stroke="hsl(43, 96%, 56%)"
                    strokeWidth={2.5}
                    dot={renderTrendDot("latency")}
                    activeDot={{ r: 6 }}
                    name="P95 Latency"
                  />
                </LineChart>
              </ChartContainer>
              <p className="mt-3 text-xs text-slate-500">
                Error points narrow the table to error records. Latency points keep all statuses.
              </p>
            </CardContent>
          </Card>
        </section>

        <Card id="usage-filters" className="border border-white/10 bg-white/[0.04]">
          <CardHeader className="gap-3">
            <div>
              <CardTitle className="text-white">Filters</CardTitle>
              <CardDescription className="text-slate-400">
                Narrow the dataset without leaving the dashboard overview.
              </CardDescription>
            </div>
            <CardAction className="self-center">
              <Button
                variant="ghost"
                size="sm"
                onClick={resetFilters}
                className="text-slate-300 hover:bg-white/10 hover:text-white"
                disabled={activeFilters.length === 0}
              >
                Reset all
              </Button>
            </CardAction>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-5">
              <Input
                placeholder="Team ID"
                value={filters.team_id}
                onChange={(event) => updateFilter("team_id", event.target.value)}
                className="border-white/10 bg-white/5 text-slate-50"
              />
              <Input
                placeholder="Router"
                value={filters.router}
                onChange={(event) => updateFilter("router", event.target.value)}
                className="border-white/10 bg-white/5 text-slate-50"
              />
              <Input
                placeholder="Channel"
                value={filters.channel}
                onChange={(event) => updateFilter("channel", event.target.value)}
                className="border-white/10 bg-white/5 text-slate-50"
              />
              <Input
                placeholder="Model"
                value={filters.model}
                onChange={(event) => updateFilter("model", event.target.value)}
                className="border-white/10 bg-white/5 text-slate-50"
              />
              <div className="flex flex-wrap gap-2 rounded-xl border border-white/10 bg-white/5 p-2">
                {[
                  { label: "All", value: "" },
                  { label: "Errors", value: "errors" },
                  { label: "Fallback Errors", value: "fallback_error" },
                  { label: "Fallbacks", value: "fallbacks" },
                ].map((option) => (
                  <Button
                    key={option.label}
                    type="button"
                    size="sm"
                    variant={filters.status === option.value ? "default" : "ghost"}
                    onClick={() => updateFilter("status", option.value)}
                    className={cn(
                      "flex-1 min-w-fit",
                      filters.status === option.value
                        ? "bg-sky-500 text-slate-950 hover:bg-sky-400"
                        : "text-slate-300 hover:bg-white/10 hover:text-white"
                    )}
                  >
                    {option.label}
                  </Button>
                ))}
              </div>
            </div>

            {activeFilters.length > 0 ? (
              <div className="flex flex-wrap gap-2">
                {activeFilters.map((filter) => (
                  <button
                    key={filter.key}
                    type="button"
                    onClick={() => removeFilterChip(filter.key)}
                    className="inline-flex items-center gap-2 rounded-full border border-sky-400/20 bg-sky-400/10 px-3 py-1 text-xs font-medium text-sky-100 transition-colors hover:bg-sky-400/20"
                  >
                    {filter.label}
                    <X className="size-3" />
                  </button>
                ))}
              </div>
            ) : (
              <p className="text-sm text-slate-500">No active filters.</p>
            )}
          </CardContent>
        </Card>

        <Card id="usage-records-table" className="border border-white/10 bg-white/[0.04]">
          <CardHeader>
            <CardTitle className="text-white">Usage Records</CardTitle>
            <CardDescription className="text-slate-400">
              Use this table to validate what happened after spotting an anomaly above.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <Table className="text-slate-100">
              <TableHeader>
                <TableRow className="border-white/10 hover:bg-transparent">
                  <TableHead className="text-slate-300">Timestamp</TableHead>
                  <TableHead className="text-slate-300">Request ID</TableHead>
                  <TableHead className="text-slate-300">Team</TableHead>
                  <TableHead className="text-slate-300">Router</TableHead>
                  <TableHead className="text-slate-300">Channel</TableHead>
                  <TableHead className="text-slate-300">Model</TableHead>
                  <TableHead className="text-right text-slate-300">Total Tokens</TableHead>
                  <TableHead className="text-right text-slate-300">Latency</TableHead>
                  <TableHead className="text-slate-300">Status</TableHead>
                  <TableHead className="text-right text-slate-300">Input</TableHead>
                  <TableHead className="text-right text-slate-300">Output</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {tableLoading
                  ? Array.from({ length: 5 }).map((_, index) => (
                      <TableRow key={index} className="border-white/5">
                        <TableCell colSpan={11} className="py-4">
                          <div className="h-4 w-full animate-pulse rounded bg-white/8" />
                        </TableCell>
                      </TableRow>
                    ))
                  : usage.length === 0 ? (
                      <TableRow className="border-white/5">
                        <TableCell colSpan={11} className="py-10 text-center text-slate-400">
                          {activeFilters.length > 0
                            ? "No records match the current filters."
                            : "No usage records are available for this time range."}
                        </TableCell>
                      </TableRow>
                    ) : (
                      usage.map((record) => {
                        const totalTokens = record.input_tokens + record.output_tokens;

                        return (
                          <TableRow
                            key={record.id}
                            className={cn(
                              "cursor-pointer border-white/5",
                              selectedRecord?.id === record.id && "bg-white/10"
                            )}
                            role="button"
                            tabIndex={0}
                            onClick={() => setSelectedRecord(record)}
                            onKeyDown={(event) => {
                              if (event.key === "Enter" || event.key === " ") {
                                event.preventDefault();
                                setSelectedRecord(record);
                              }
                            }}
                          >
                            <TableCell className="font-mono text-xs text-slate-300">
                              {record.timestamp}
                            </TableCell>
                            <TableCell className="font-mono text-xs text-slate-400">
                              {record.request_id ?? "-"}
                            </TableCell>
                            <TableCell>{record.team_id}</TableCell>
                            <TableCell>{record.router}</TableCell>
                            <TableCell>{record.channel}</TableCell>
                            <TableCell>{record.model}</TableCell>
                            <TableCell className="text-right font-medium">
                              {formatNumber(totalTokens)}
                            </TableCell>
                            <TableCell className="text-right text-slate-300">
                              {record.latency_ms != null ? `${formatLatency(record.latency_ms)} ms` : "-"}
                            </TableCell>
                            <TableCell>
                              <span
                                className={cn(
                                  "inline-flex rounded-full px-2 py-1 text-xs font-medium",
                                  record.fallback_triggered
                                    ? "bg-amber-400/15 text-amber-200"
                                    : record.status === "error" || record.status === "fallback_error"
                                      ? "bg-rose-400/15 text-rose-200"
                                      : "bg-emerald-400/15 text-emerald-200"
                                )}
                              >
                                {record.status === "fallback_error"
                                  ? "Fallback Error"
                                  : record.status === "error"
                                    ? "Error"
                                    : record.fallback_triggered
                                      ? "Fallback"
                                      : "Success"}
                              </span>
                            </TableCell>
                            <TableCell className="text-right text-slate-300">
                              {formatNumber(record.input_tokens)}
                            </TableCell>
                            <TableCell className="text-right text-slate-300">
                              {formatNumber(record.output_tokens)}
                            </TableCell>
                          </TableRow>
                        );
                      })
                    )}
              </TableBody>
            </Table>

            <div className="flex flex-col gap-3 border-t border-white/10 pt-4 text-sm text-slate-400 sm:flex-row sm:items-center sm:justify-between">
              <div>
                Showing {showingStart}-{showingEnd} of {total} records
              </div>
              <div className="flex gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => goToPage("prev")}
                  disabled={pagination.offset === 0}
                  className="border-white/10 bg-white/5 text-slate-100 hover:bg-white/10"
                >
                  Previous
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => goToPage("next")}
                  disabled={usage.length < pagination.limit}
                  className="border-white/10 bg-white/5 text-slate-100 hover:bg-white/10"
                >
                  Next
                </Button>
              </div>
            </div>
          </CardContent>
        </Card>
      </div>

      {selectedRecord ? (
        <div className="pointer-events-none fixed inset-0 z-50 flex justify-end bg-slate-950/45 p-0">
          <div className="pointer-events-auto h-full w-full max-w-md overflow-y-auto border-l border-white/10 bg-slate-950/95 p-6 shadow-2xl shadow-slate-950/60">
            <div className="mb-6 flex items-start justify-between gap-4">
              <div>
                <p className="text-xs font-medium uppercase tracking-[0.22em] text-sky-300/70">
                  Usage details
                </p>
                <h2 className="mt-2 text-2xl font-semibold text-white">
                  {selectedRecord.model}
                </h2>
                <p className="mt-1 text-sm text-slate-400">
                  {selectedRecord.timestamp}
                </p>
              </div>
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => setSelectedRecord(null)}
                className="text-slate-300 hover:bg-white/10 hover:text-white"
              >
                <X className="size-4" />
              </Button>
            </div>

            <div className="space-y-4">
              <DetailGroup
                title="Request"
                rows={[
                  ["Request ID", selectedRecord.request_id ?? "Not available"],
                  ["Status", selectedRecord.status],
                  [
                    "Status Code",
                    selectedRecord.status_code != null
                      ? String(selectedRecord.status_code)
                      : "Not available",
                  ],
                ]}
              />
              <DetailGroup
                title="Routing"
                rows={[
                  ["Team", selectedRecord.team_id],
                  ["Router", selectedRecord.router],
                  ["Channel", selectedRecord.channel],
                ]}
              />
              <DetailGroup
                title="Tokens"
                rows={[
                  ["Input Tokens", formatNumber(selectedRecord.input_tokens)],
                  ["Output Tokens", formatNumber(selectedRecord.output_tokens)],
                  [
                    "Total Tokens",
                    formatNumber(selectedRecord.input_tokens + selectedRecord.output_tokens),
                  ],
                ]}
              />
              <DetailGroup
                title="Availability"
                rows={[
                  [
                    "Latency",
                    selectedRecord.latency_ms != null
                      ? `${formatLatency(selectedRecord.latency_ms)} ms`
                      : "Not recorded",
                  ],
                  [
                    "Fallback",
                    selectedRecord.fallback_triggered ? "Triggered" : "Not triggered",
                  ],
                ]}
              />
              {selectedRecord.error_message ? (
                <DetailGroup
                  title="Error"
                  rows={[["Message", selectedRecord.error_message]]}
                />
              ) : null}
              {selectedRecord.provider_trace_id || selectedRecord.provider_error_body ? (
                <DetailGroup
                  title="Provider diagnostics"
                  rows={[
                    [
                      "Trace ID",
                      selectedRecord.provider_trace_id ?? "Not available",
                    ],
                    [
                      "Error Body",
                      selectedRecord.provider_error_body ?? "Not available",
                    ],
                  ]}
                />
              ) : null}
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}

function MetricCard({
  label,
  value,
  tone,
  hint,
  actionLabel,
  onClick,
}: {
  label: string;
  value: string;
  tone: "neutral" | "healthy" | "warning" | "critical";
  hint: string;
  actionLabel?: string;
  onClick?: () => void;
}) {
  const toneClasses =
    tone === "healthy"
      ? "border-emerald-400/15 bg-emerald-400/[0.07]"
      : tone === "warning"
        ? "border-amber-400/15 bg-amber-400/[0.08]"
        : tone === "critical"
          ? "border-rose-400/15 bg-rose-400/[0.08]"
          : "border-white/10 bg-white/[0.04]";

  const content = (
    <>
      <CardHeader className="pb-1">
        <CardTitle className="text-sm font-medium text-slate-300">{label}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-2">
        <div className="text-3xl font-semibold tracking-tight text-white">{value}</div>
        <p className="text-xs leading-5 text-slate-400">{hint}</p>
        {actionLabel ? <p className="text-xs font-medium text-sky-300/80">{actionLabel}</p> : null}
      </CardContent>
    </>
  );

  if (onClick) {
    return (
      <Card className={cn("overflow-hidden border", toneClasses)}>
        <button
          type="button"
          onClick={onClick}
          className="block w-full text-left transition-colors hover:bg-white/[0.03] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400/70"
          aria-label={`Inspect usage records from ${label}`}
        >
          {content}
        </button>
      </Card>
    );
  }

  return <Card className={cn("overflow-hidden border", toneClasses)}>{content}</Card>;
}

function DetailGroup({
  title,
  rows,
}: {
  title: string;
  rows: Array<[string, string]>;
}) {
  return (
    <Card className="border border-white/10 bg-white/[0.04]">
      <CardHeader>
        <CardTitle className="text-sm font-medium text-slate-200">{title}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        {rows.map(([label, value]) => (
          <div key={label} className="flex items-start justify-between gap-4 text-sm">
            <span className="text-slate-400">{label}</span>
            <span className="text-right text-slate-100">{value}</span>
          </div>
        ))}
      </CardContent>
    </Card>
  );
}
