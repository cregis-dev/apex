"use client";

import { Area, AreaChart, Bar, BarChart, CartesianGrid, Tooltip, XAxis, YAxis } from "recharts";

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { ChartContainer, ChartTooltipContent, type ChartConfig } from "@/components/ui/chart";
import type { DashboardAnalyticsResponse } from "@/components/dashboard/types";

const errorConfig: ChartConfig = {
  error_rate: { label: "Error Rate", color: "#ef4444" },
};

const latencyConfig: ChartConfig = {
  avg_latency_ms: { label: "Avg Latency", color: "#94a3b8" },
};

type SystemReliabilityTabProps = {
  analytics: DashboardAnalyticsResponse | null;
};

export function SystemReliabilityTab({ analytics }: SystemReliabilityTabProps) {
  if (!analytics) {
    return null;
  }

  return (
    <div className="grid gap-6 xl:grid-cols-2">
      <Card className="rounded-[22px] border-[#111827] bg-white">
        <CardHeader>
          <CardTitle>Error Rate Trend</CardTitle>
          <CardDescription>Failure percentage over time for the current gateway slice.</CardDescription>
        </CardHeader>
        <CardContent>
          <ChartContainer className="h-[320px] w-full" config={errorConfig}>
            <AreaChart data={analytics.system_reliability.error_rate_trend}>
              <CartesianGrid vertical={false} />
              <XAxis dataKey="label" tickLine={false} axisLine={false} />
              <YAxis tickLine={false} axisLine={false} unit="%" />
              <Tooltip content={<ChartTooltipContent />} />
              <Area
                type="monotone"
                dataKey="error_rate"
                stroke="var(--color-error_rate)"
                fill="var(--color-error_rate)"
                fillOpacity={0.18}
              />
            </AreaChart>
          </ChartContainer>
        </CardContent>
      </Card>

      <Card className="rounded-[22px] border-[#111827] bg-white">
        <CardHeader>
          <CardTitle>Channel Latency Comparison</CardTitle>
          <CardDescription>Average response latency by final delivery channel.</CardDescription>
        </CardHeader>
        <CardContent>
          <ChartContainer className="h-[320px] w-full" config={latencyConfig}>
            <BarChart data={analytics.system_reliability.channel_latency} layout="vertical" margin={{ left: 16 }}>
              <CartesianGrid horizontal={false} />
              <XAxis type="number" tickLine={false} axisLine={false} />
              <YAxis type="category" width={120} dataKey="channel" tickLine={false} axisLine={false} />
              <Tooltip content={<ChartTooltipContent />} />
              <Bar dataKey="avg_latency_ms" fill="var(--color-avg_latency_ms)" radius={[0, 10, 10, 0]} />
            </BarChart>
          </ChartContainer>
        </CardContent>
      </Card>
    </div>
  );
}
