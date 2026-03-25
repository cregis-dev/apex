"use client";

import { Bar, BarChart, CartesianGrid, Tooltip, XAxis, YAxis } from "recharts";

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { ChartContainer, ChartTooltipContent, type ChartConfig } from "@/components/ui/chart";
import type { DashboardAnalyticsResponse } from "@/components/dashboard/types";

const leaderboardConfig: ChartConfig = {
  total_tokens: { label: "Total Tokens", color: "#94a3b8" },
};

const usageConfig: ChartConfig = {
  total_tokens: { label: "Total Tokens", color: "#64748b" },
};
const STACK_COLORS = ["#64748b", "#94a3b8", "#cbd5e1", "#1e293b", "#475569", "#0f172a"];

type TeamUsageTabProps = {
  analytics: DashboardAnalyticsResponse | null;
};

const TEAM_LEADERBOARD_LIMIT = 10;

export function TeamUsageTab({ analytics }: TeamUsageTabProps) {
  if (!analytics) {
    return null;
  }

  const leaderboard = analytics.team_usage.leaderboard.slice(0, TEAM_LEADERBOARD_LIMIT);
  const leaderboardHeight = Math.max(320, leaderboard.length * 52 + 60);
  const models = Array.from(
    new Set(analytics.team_usage.model_usage.map((item) => item.model))
  );
  const stackedUsage = analytics.team_usage.model_usage.reduce<Array<Record<string, string | number>>>(
    (acc, item) => {
      const existing = acc.find((entry) => entry.team_id === item.team_id);
      if (existing) {
        existing[item.model] = item.total_tokens;
        return acc;
      }

      acc.push({
        team_id: item.team_id,
        [item.model]: item.total_tokens,
      });
      return acc;
    },
    []
  );

  return (
    <div className="grid gap-6 xl:grid-cols-2">
      <Card className="rounded-[22px] border-[#111827] bg-white">
        <CardHeader>
          <CardTitle>Team Leaderboard</CardTitle>
          <CardDescription>Top 10 teams ranked by token consumption.</CardDescription>
        </CardHeader>
        <CardContent>
          <ChartContainer
            className="w-full"
            config={leaderboardConfig}
            style={{ height: `${leaderboardHeight}px` }}
          >
            <BarChart data={leaderboard} layout="vertical" margin={{ left: 16 }}>
              <CartesianGrid horizontal={false} />
              <XAxis type="number" hide />
              <YAxis type="category" width={110} dataKey="team_id" tickLine={false} axisLine={false} />
              <Tooltip content={<ChartTooltipContent />} />
              <Bar dataKey="total_tokens" fill="var(--color-total_tokens)" radius={[0, 10, 10, 0]} />
            </BarChart>
          </ChartContainer>
        </CardContent>
      </Card>

      <Card className="rounded-[22px] border-[#111827] bg-white">
        <CardHeader>
          <CardTitle>Model Usage by Team</CardTitle>
          <CardDescription>Token distribution across models inside each team.</CardDescription>
        </CardHeader>
        <CardContent>
          <ChartContainer className="h-[320px] w-full" config={usageConfig}>
            <BarChart data={stackedUsage}>
              <CartesianGrid vertical={false} />
              <XAxis dataKey="team_id" tickLine={false} axisLine={false} />
              <YAxis tickLine={false} axisLine={false} />
              <Tooltip content={<ChartTooltipContent />} />
              {models.map((item, index) => (
                <Bar
                  key={item}
                  dataKey={item}
                  stackId="usage"
                  fill={STACK_COLORS[index % STACK_COLORS.length]}
                  radius={index === models.length - 1 ? [10, 10, 0, 0] : [0, 0, 0, 0]}
                />
              ))}
            </BarChart>
          </ChartContainer>
        </CardContent>
      </Card>
    </div>
  );
}
