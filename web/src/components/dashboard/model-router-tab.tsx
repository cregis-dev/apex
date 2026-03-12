"use client";

import { Pie, PieChart, Cell, Tooltip } from "recharts";

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { ChartContainer, ChartTooltipContent, type ChartConfig } from "@/components/ui/chart";
import type { DashboardAnalyticsResponse } from "@/components/dashboard/types";

const PIE_COLORS = ["#64748b", "#94a3b8", "#cbd5e1", "#1e293b", "#475569", "#0f172a"];

const chartConfig: ChartConfig = {
  requests: { label: "Requests", color: "#64748b" },
};

type ModelRouterTabProps = {
  analytics: DashboardAnalyticsResponse | null;
};

function SummaryList({
  title,
  description,
  items,
}: {
  title: string;
  description: string;
  items: DashboardAnalyticsResponse["model_router"]["router_summary"];
}) {
  return (
    <Card className="rounded-[22px] border-[#111827] bg-white">
      <CardHeader>
        <CardTitle>{title}</CardTitle>
        <CardDescription>{description}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        {items.length === 0 ? (
          <div className="rounded-2xl border border-dashed border-[#d5dce5] bg-[#f8fafc] px-4 py-8 text-sm text-[#64748b]">
            No data available.
          </div>
        ) : (
          items.slice(0, 6).map((item) => (
            <div key={item.name} className="flex items-center justify-between rounded-2xl bg-[#f8fafc] px-4 py-3">
              <div>
                <div className="font-medium text-[#17233c]">{item.name}</div>
                <div className="text-xs text-[#64748b]">{item.total_tokens.toLocaleString()} tokens</div>
              </div>
              <div className="text-right text-sm">
                <div className="font-medium text-[#17233c]">{item.requests.toLocaleString()}</div>
                <div className="text-xs text-[#64748b]">{item.percentage.toFixed(1)}%</div>
              </div>
            </div>
          ))
        )}
      </CardContent>
    </Card>
  );
}

export function ModelRouterTab({ analytics }: ModelRouterTabProps) {
  if (!analytics) {
    return null;
  }

  return (
    <div className="grid gap-6 xl:grid-cols-[1.1fr_0.9fr_0.9fr]">
      <Card className="rounded-[22px] border-[#111827] bg-white">
        <CardHeader>
          <CardTitle>Model Share</CardTitle>
          <CardDescription>Request share by model with hover-level request and percentage detail.</CardDescription>
        </CardHeader>
        <CardContent>
          <ChartContainer className="h-[320px] w-full" config={chartConfig}>
            <PieChart>
              <Tooltip content={<ChartTooltipContent />} />
              <Pie
                data={analytics.model_router.model_share}
                dataKey="requests"
                nameKey="name"
                innerRadius={68}
                outerRadius={112}
                paddingAngle={3}
              >
                {analytics.model_router.model_share.map((item, index) => (
                  <Cell key={item.name} fill={PIE_COLORS[index % PIE_COLORS.length]} />
                ))}
              </Pie>
            </PieChart>
          </ChartContainer>
        </CardContent>
      </Card>

      <SummaryList
        title="Router Summary"
        description="Which router families carry the most requests."
        items={analytics.model_router.router_summary}
      />

      <SummaryList
        title="Channel Summary"
        description="Final channels after routing and fallback resolution."
        items={analytics.model_router.channel_summary}
      />
    </div>
  );
}
