"use client";

import { ArrowDownRight, ArrowUpRight } from "lucide-react";
import { Bar, CartesianGrid, ComposedChart, Line, Tooltip, XAxis, YAxis } from "recharts";

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { ChartContainer, ChartTooltipContent, type ChartConfig } from "@/components/ui/chart";
import type { DashboardAnalyticsResponse } from "@/components/dashboard/types";

const chartConfig: ChartConfig = {
  requests: { label: "Requests", color: "#8e3f1d" },
  total_tokens: { label: "Tokens", color: "#d39a2f" },
};

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
              Flow summary fallback for Team → Router → Channel → Model routing.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-3">
            {analytics.topology.flows.length === 0 ? (
              <div className="rounded-2xl border border-dashed border-[#d5dce5] bg-[#f8fafc] px-4 py-8 text-sm text-[#64748b]">
                No topology flows found for the current filters.
              </div>
            ) : (
              analytics.topology.flows.slice(0, 6).map((flow) => (
                <div
                  key={`${flow.team_id}-${flow.router}-${flow.channel}-${flow.model}`}
                  className="rounded-2xl border border-[#d8dee7] bg-[#f8fafc] px-4 py-3"
                >
                  <div className="flex items-center justify-between gap-3">
                    <div className="text-sm font-medium text-[#17233c]">
                      {flow.team_id} → {flow.router}
                    </div>
                    <div className="text-xs text-[#64748b]">{flow.requests.toLocaleString()} req</div>
                  </div>
                  <div className="mt-2 text-sm text-[#475569]">
                    {flow.channel} → {flow.model}
                  </div>
                  <div className="mt-2 text-xs text-[#64748b]">
                    {flow.total_tokens.toLocaleString()} total tokens
                  </div>
                </div>
              ))
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
