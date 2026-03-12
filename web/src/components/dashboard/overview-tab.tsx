"use client";

import { ArrowDownRight, ArrowUpRight } from "lucide-react";
import {
  Bar,
  CartesianGrid,
  ComposedChart,
  Line,
  ResponsiveContainer,
  Sankey,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { ChartContainer, ChartTooltipContent, type ChartConfig } from "@/components/ui/chart";
import type { DashboardAnalyticsResponse } from "@/components/dashboard/types";

const chartConfig: ChartConfig = {
  requests: { label: "Requests", color: "#8e3f1d" },
  total_tokens: { label: "Tokens", color: "#d39a2f" },
};

const TOPOLOGY_NODE_COLORS: Record<string, string> = {
  team: "#facc15",
  router: "#fb923c",
  channel: "#f97316",
  model: "#ea580c",
};

const TOPOLOGY_COLUMNS = ["Team", "Router", "Channel", "Model"];

type OverviewTabProps = {
  analytics: DashboardAnalyticsResponse | null;
};

function formatCompact(value: number) {
  return new Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 }).format(value);
}

function DeltaPill({ value }: { value: number }) {
  const positive = value >= 0;
  const Icon = positive ? ArrowUpRight : ArrowDownRight;

  return (
    <div
      className={`inline-flex items-center gap-1 rounded-full px-2 py-1 text-xs font-medium ${
        positive ? "text-[#059669]" : "text-[#ef4444]"
      }`}
    >
      <Icon className="size-3.5" />
      {Math.abs(value).toFixed(1)}%
    </div>
  );
}

function formatTopologyValue(value: number) {
  return `${value.toLocaleString()} requests`;
}

function TopologyNode(props: {
  x?: number;
  y?: number;
  width?: number;
  height?: number;
  payload?: { kind?: string };
}) {
  const { x = 0, y = 0, width = 0, height = 0, payload } = props;
  const kind = payload?.kind ?? "team";
  const fill = TOPOLOGY_NODE_COLORS[kind] ?? "#f97316";

  return (
    <g>
      <rect
        x={x}
        y={y}
        width={width}
        height={height}
        rx={2}
        fill={fill}
        fillOpacity={0.72}
        stroke="#f97316"
        strokeWidth={1.5}
      />
    </g>
  );
}

function TopologyLink(props: {
  sourceX?: number;
  sourceY?: number;
  sourceControlX?: number;
  targetX?: number;
  targetY?: number;
  targetControlX?: number;
  linkWidth?: number;
}) {
  const {
    sourceX = 0,
    sourceY = 0,
    sourceControlX = 0,
    targetX = 0,
    targetY = 0,
    targetControlX = 0,
    linkWidth = 0,
  } = props;

  return (
    <g>
      <path
        d={`M${sourceX},${sourceY} C${sourceControlX},${sourceY} ${targetControlX},${targetY} ${targetX},${targetY}`}
        fill="none"
        stroke="#f4d8b5"
        strokeOpacity={0.92}
        strokeWidth={linkWidth}
        strokeLinecap="round"
      />
      <path
        d={`M${sourceX},${sourceY} C${sourceControlX},${sourceY} ${targetControlX},${targetY} ${targetX},${targetY}`}
        fill="none"
        stroke="#efc89a"
        strokeOpacity={0.55}
        strokeWidth={Math.max(linkWidth - 2, 1)}
        strokeLinecap="round"
      />
    </g>
  );
}

function TopologyTooltip(props: {
  active?: boolean;
  payload?: Array<{
    name?: string;
    value?: number;
    payload?: {
      value?: number;
      payload?: {
        name?: string;
        kind?: string;
        source?: { name?: string };
        target?: { name?: string };
      };
    };
  }>;
}) {
  const item = props.payload?.[0];
  if (!props.active || !item) {
    return null;
  }

  const raw = item.payload?.payload;
  const value = item.value ?? item.payload?.value ?? 0;
  const sourceName = raw?.source?.name;
  const targetName = raw?.target?.name;

  return (
    <div className="rounded-xl border border-[#d6dde8] bg-white px-3 py-2 shadow-[0_12px_24px_rgba(15,23,42,0.12)]">
      <div className="text-sm font-semibold text-[#17233c]">
        {sourceName && targetName ? `${sourceName} → ${targetName}` : item.name}
      </div>
      <div className="mt-1 text-xs text-[#64748b]">{formatTopologyValue(value)}</div>
    </div>
  );
}

export function OverviewTab({ analytics }: OverviewTabProps) {
  if (!analytics) {
    return (
      <Card>
        <CardContent className="py-10 text-sm text-muted-foreground">
          No overview data available for the current filters.
        </CardContent>
      </Card>
    );
  }

  const kpis = [
    {
      label: "Total Requests",
      value: analytics.overview.total_requests.toLocaleString(),
      delta: analytics.overview.delta.total_requests,
      meta: "",
    },
    {
      label: "Total Tokens",
      value: formatCompact(analytics.overview.total_tokens),
      delta: analytics.overview.delta.total_tokens,
      meta: `In: ${analytics.overview.input_tokens.toLocaleString()} | Out: ${analytics.overview.output_tokens.toLocaleString()}`,
    },
    {
      label: "Avg Latency",
      value: `${analytics.overview.avg_latency_ms.toFixed(1)} ms`,
      delta: analytics.overview.delta.avg_latency_ms,
      meta: "",
    },
    {
      label: "Success Rate",
      value: `${analytics.overview.success_rate.toFixed(2)}%`,
      delta: analytics.overview.delta.success_rate,
      meta: "",
    },
  ];

  return (
    <div className="space-y-6">
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        {kpis.map((item) => (
          <Card key={item.label} className="rounded-[20px] border-[#111827] bg-white">
            <CardHeader className="pb-1">
              <CardDescription className="text-[15px] text-[#64748b]">{item.label}</CardDescription>
              <CardTitle className="text-[42px] tracking-tight text-black">{item.value}</CardTitle>
            </CardHeader>
            <CardContent className="space-y-2">
              <div className="flex items-center gap-2">
                <DeltaPill value={item.delta} />
                <span className="text-[13px] text-[#64748b]">vs last period</span>
              </div>
              {item.meta ? <div className="text-[13px] text-[#64748b]">{item.meta}</div> : null}
            </CardContent>
          </Card>
        ))}
      </div>

      <div className="space-y-6">
        <Card className="rounded-[22px] border-[#111827] bg-white">
          <CardHeader>
            <CardTitle>Global Trend</CardTitle>
            <CardDescription>
              Requests and token consumption across the selected dashboard window.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <ChartContainer className="h-[320px] w-full" config={chartConfig}>
              <ComposedChart data={analytics.trend.points}>
                <CartesianGrid vertical={false} />
                <XAxis dataKey="label" tickLine={false} axisLine={false} />
                <YAxis
                  yAxisId="requests"
                  tickLine={false}
                  axisLine={false}
                  width={44}
                  allowDecimals={false}
                />
                <YAxis
                  yAxisId="tokens"
                  orientation="right"
                  tickLine={false}
                  axisLine={false}
                  width={52}
                />
                <Tooltip content={<ChartTooltipContent />} />
                <Bar yAxisId="requests" dataKey="requests" fill="var(--color-requests)" radius={[8, 8, 0, 0]} />
                <Line
                  yAxisId="tokens"
                  type="monotone"
                  dataKey="total_tokens"
                  stroke="var(--color-total_tokens)"
                  strokeWidth={2.5}
                  dot={false}
                />
              </ComposedChart>
            </ChartContainer>
          </CardContent>
        </Card>

        <Card className="rounded-[22px] border-[#111827] bg-white">
          <CardHeader>
            <CardTitle>Traffic Topology</CardTitle>
            <CardDescription>
              Team → Router → Channel → Model routing across the selected window.
            </CardDescription>
          </CardHeader>
          <CardContent>
            {analytics.topology.nodes.length === 0 || analytics.topology.links.length === 0 ? (
              <div className="rounded-2xl border border-dashed border-[#d5dce5] bg-[#f8fafc] px-4 py-8 text-sm text-[#64748b]">
                No topology flows found for the current filters.
              </div>
            ) : (
              <div className="rounded-[24px] border border-[#e2e8f0] bg-white p-4">
                <div className="grid grid-cols-4 gap-4 px-6 pb-3 text-[11px] font-semibold uppercase tracking-[0.18em] text-[#94a3b8]">
                  {TOPOLOGY_COLUMNS.map((column) => (
                    <div key={column} className="text-center">
                      {column}
                    </div>
                  ))}
                </div>
                <div className="h-[332px] w-full">
                  <ResponsiveContainer width="100%" height="100%">
                    <Sankey
                      data={{
                        nodes: analytics.topology.nodes,
                        links: analytics.topology.links,
                      }}
                      node={<TopologyNode />}
                      link={<TopologyLink />}
                      nodePadding={28}
                      nodeWidth={14}
                      linkCurvature={0.52}
                      margin={{ top: 10, right: 34, bottom: 10, left: 34 }}
                      sort={false}
                    >
                      <Tooltip content={<TopologyTooltip />} />
                    </Sankey>
                  </ResponsiveContainer>
                </div>
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
