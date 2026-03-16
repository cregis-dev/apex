"use client";

import { ArrowDownRight, ArrowUpRight } from "lucide-react";
import { useState } from "react";
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

type TopologyNodePayload = DashboardAnalyticsResponse["topology"]["nodes"][number] & {
  depth?: number;
  value?: number;
};

type TopologyLinkPayload = DashboardAnalyticsResponse["topology"]["links"][number] & {
  source?: TopologyNodePayload;
  target?: TopologyNodePayload;
};

type TopologyMetrics = {
  requests: number;
  total_tokens: number;
};

type TopologyNodeRendererProps = {
  x?: number;
  y?: number;
  width?: number;
  height?: number;
  payload?: TopologyNodePayload;
};

type TopologyLinkRendererProps = {
  sourceX?: number;
  sourceY?: number;
  sourceControlX?: number;
  targetX?: number;
  targetY?: number;
  targetControlX?: number;
  linkWidth?: number;
  payload?: TopologyLinkPayload;
};

type TopologyHoverOverlay =
  | {
      type: "node";
      title: string;
      kind: string;
      requests: number;
      total_tokens: number;
      x: number;
      y: number;
      revision: string;
    }
  | {
      type: "link";
      title: string;
      requests: number;
      total_tokens: number;
      x: number;
      y: number;
      revision: string;
    };

function topologySelectorValue(value: string) {
  return encodeURIComponent(value);
}

function formatCompact(value: number) {
  return new Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 }).format(value);
}

function formatInteger(value: number) {
  return value.toLocaleString("en");
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
  return `${formatInteger(value)} requests`;
}

function topologyMetricKey(kind: string, name: string) {
  return `${kind}::${name}`;
}

function buildTopologyNodeMetrics(
  flows: DashboardAnalyticsResponse["topology"]["flows"]
): Map<string, TopologyMetrics> {
  const metrics = new Map<string, TopologyMetrics>();

  for (const flow of flows) {
    for (const [kind, name] of [
      ["team", flow.team_id],
      ["router", flow.router],
      ["channel", flow.channel],
      ["model", flow.model],
    ] as const) {
      const key = topologyMetricKey(kind, name);
      const current = metrics.get(key) ?? { requests: 0, total_tokens: 0 };
      current.requests += flow.requests;
      current.total_tokens += flow.total_tokens;
      metrics.set(key, current);
    }
  }

  return metrics;
}

function truncateLabel(value: string, maxChars: number) {
  if (maxChars <= 0) {
    return { text: "", truncated: value.length > 0 };
  }

  if (value.length <= maxChars) {
    return { text: value, truncated: false };
  }

  if (maxChars <= 1) {
    return { text: "…", truncated: true };
  }

  return { text: `${value.slice(0, maxChars - 1)}…`, truncated: true };
}

function formatTopologyKind(kind: string) {
  return kind.charAt(0).toUpperCase() + kind.slice(1);
}

function getNodeLabelCapacity(depth: number, height: number) {
  if (height < 16) {
    return 0;
  }

  const availableWidth = depth === 0 || depth === 3 ? 128 : 88;
  return Math.floor(availableWidth / 7);
}

function buildTopologyLinkPath(props: {
  sourceX: number;
  sourceY: number;
  sourceControlX: number;
  targetX: number;
  targetY: number;
  targetControlX: number;
}) {
  const { sourceX, sourceY, sourceControlX, targetX, targetY, targetControlX } = props;
  return `M${sourceX},${sourceY} C${sourceControlX},${sourceY} ${targetControlX},${targetY} ${targetX},${targetY}`;
}

function TopologyNode(props: TopologyNodeRendererProps & {
  metrics?: TopologyMetrics;
  revision: string;
  onHoverChange?: (tooltip: TopologyHoverOverlay | null) => void;
}) {
  const { x = 0, y = 0, width = 0, height = 0, payload, metrics, revision, onHoverChange } = props;
  const kind = payload?.kind ?? "team";
  const depth = payload?.depth ?? 0;
  const fill = TOPOLOGY_NODE_COLORS[kind] ?? "#f97316";
  const labelSource = payload?.name ?? "";
  const canShowLabel = labelSource.length > 0 && height >= 16;
  const maxChars = getNodeLabelCapacity(depth, height);
  const { text: nodeLabel } = truncateLabel(labelSource, maxChars);
  const labelX =
    depth === 0 ? x + width + 8 : depth === 3 ? x - 8 : x + width / 2;
  const labelAnchor = depth === 0 ? "start" : depth === 3 ? "end" : "middle";
  const metricSummary =
    metrics && height >= 34 ? `${formatInteger(metrics.requests)} req` : "";
  const requests = metrics?.requests ?? payload?.value ?? 0;
  const totalTokens = metrics?.total_tokens ?? 0;

  const handleMouseEnter = () => {
    if (!payload?.name) {
      return;
    }

    onHoverChange?.({
      type: "node",
      title: payload.name,
      kind,
      requests,
      total_tokens: totalTokens,
      x: x + width / 2,
      y: y + height / 2,
      revision,
    });
  };

  return (
    <g
      className="recharts-sankey-node"
      data-topology-node={topologySelectorValue(payload?.name ?? "")}
      data-topology-kind={kind}
      onMouseEnter={handleMouseEnter}
      onFocus={handleMouseEnter}
      onClick={handleMouseEnter}
      onMouseLeave={() => onHoverChange?.(null)}
      onBlur={() => onHoverChange?.(null)}
      tabIndex={0}
      role="button"
    >
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
      {canShowLabel && nodeLabel ? (
        <text
          x={labelX}
          y={y + height / 2 - (metricSummary ? 7 : 0)}
          fill="#17233c"
          fontSize={12}
          fontWeight={600}
          textAnchor={labelAnchor}
          dominantBaseline="middle"
          pointerEvents="none"
        >
          {nodeLabel}
        </text>
      ) : null}
      {metricSummary ? (
        <text
          x={labelX}
          y={y + height / 2 + 9}
          fill="#64748b"
          fontSize={10}
          textAnchor={labelAnchor}
          dominantBaseline="middle"
          pointerEvents="none"
        >
          {metricSummary}
        </text>
      ) : null}
    </g>
  );
}

function TopologyLink(props: TopologyLinkRendererProps & {
  revision: string;
  onHoverChange?: (tooltip: TopologyHoverOverlay | null) => void;
}) {
  const {
    sourceX = 0,
    sourceY = 0,
    sourceControlX = 0,
    targetX = 0,
    targetY = 0,
    targetControlX = 0,
    linkWidth = 0,
    payload,
    revision,
    onHoverChange,
  } = props;
  const path = buildTopologyLinkPath({
    sourceX,
    sourceY,
    sourceControlX,
    targetX,
    targetY,
    targetControlX,
  });
  const sourceName = payload?.source?.name ?? "";
  const targetName = payload?.target?.name ?? "";
  const labelSource = sourceName && targetName ? `${sourceName} → ${targetName}` : "";
  const span = Math.max(targetX - sourceX, 0);
  const approxChars = Math.floor((span - 24) / 7);
  const { text: fullPathLabel, truncated } = truncateLabel(labelSource, approxChars);
  const summaryLabel = `${formatInteger(payload?.value ?? 0)} req`;
  const canShowPathLabel = labelSource.length > 0 && linkWidth >= 14 && span >= 220 && approxChars >= 14;
  const canShowSummary = !canShowPathLabel && linkWidth >= 12 && span >= 132;
  const label = canShowPathLabel ? fullPathLabel : canShowSummary ? summaryLabel : "";
  const labelOpacity = truncated ? 0.92 : 1;
  const handleMouseEnter = () => {
    if (!labelSource) {
      return;
    }

    onHoverChange?.({
      type: "link",
      title: labelSource,
      requests: payload?.value ?? 0,
      total_tokens: payload?.total_tokens ?? 0,
      x: (sourceX + targetX) / 2,
      y: (sourceY + targetY) / 2,
      revision,
    });
  };

  return (
    <g
      data-topology-link={topologySelectorValue(labelSource)}
      onMouseEnter={handleMouseEnter}
      onFocus={handleMouseEnter}
      onClick={handleMouseEnter}
      onMouseLeave={() => onHoverChange?.(null)}
      onBlur={() => onHoverChange?.(null)}
      tabIndex={0}
      role="button"
    >
      <path
        d={path}
        fill="none"
        stroke="#f4d8b5"
        strokeOpacity={0.92}
        strokeWidth={linkWidth}
        strokeLinecap="round"
      />
      <path
        d={path}
        fill="none"
        stroke="#efc89a"
        strokeOpacity={0.55}
        strokeWidth={Math.max(linkWidth - 2, 1)}
        strokeLinecap="round"
      />
      {label ? (
        <text
          x={(sourceX + targetX) / 2}
          y={(sourceY + targetY) / 2}
          fill="#8e3f1d"
          fontSize={11}
          fontWeight={600}
          textAnchor="middle"
          dominantBaseline="middle"
          opacity={labelOpacity}
          pointerEvents="none"
        >
          {label}
        </text>
      ) : null}
    </g>
  );
}

export function OverviewTab({ analytics }: OverviewTabProps) {
  const [topologyTooltip, setTopologyTooltip] = useState<TopologyHoverOverlay | null>(null);

  if (!analytics) {
    return (
      <Card>
        <CardContent className="py-10 text-sm text-muted-foreground">
          No overview data available for the current filters.
        </CardContent>
      </Card>
    );
  }

  const topologyNodeMetrics = buildTopologyNodeMetrics(analytics.topology.flows);
  const topologyRevision = `${analytics.generated_at}:${analytics.topology.nodes.length}:${analytics.topology.links.length}`;

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
                  <div className="relative h-full w-full" onMouseLeave={() => setTopologyTooltip(null)}>
                    {topologyTooltip && topologyTooltip.revision === topologyRevision ? (
                      <div
                        className="pointer-events-none absolute z-10 rounded-xl border border-[#d6dde8] bg-white px-3 py-2 shadow-[0_12px_24px_rgba(15,23,42,0.12)]"
                        data-topology-tooltip={topologyTooltip.type}
                        style={{
                          left: `clamp(16px, ${topologyTooltip.x}px, calc(100% - 16px))`,
                          top: topologyTooltip.y < 72 ? topologyTooltip.y + 16 : topologyTooltip.y,
                          transform:
                            topologyTooltip.y < 72
                              ? "translate(-50%, 0)"
                              : "translate(-50%, calc(-100% - 12px))",
                          maxWidth: "min(320px, calc(100% - 32px))",
                        }}
                      >
                        <div className="text-sm font-semibold text-[#17233c]" data-topology-tooltip-title>
                          {topologyTooltip.title}
                        </div>
                        {topologyTooltip.type === "node" ? (
                          <div
                            className="mt-1 text-xs uppercase tracking-[0.18em] text-[#94a3b8]"
                            data-topology-tooltip-kind
                          >
                            {formatTopologyKind(topologyTooltip.kind)}
                          </div>
                        ) : null}
                        <div className="mt-2 grid gap-1 text-xs text-[#64748b]">
                          <div>{formatTopologyValue(topologyTooltip.requests)}</div>
                          <div>{`${formatInteger(topologyTooltip.total_tokens)} total tokens`}</div>
                        </div>
                      </div>
                    ) : null}
                  <ResponsiveContainer width="100%" height="100%">
                    <Sankey
                      data={{
                        nodes: analytics.topology.nodes,
                        links: analytics.topology.links,
                      }}
                      node={(nodeProps: TopologyNodeRendererProps) => (
                        <TopologyNode
                          {...nodeProps}
                          metrics={
                            nodeProps.payload
                              ? topologyNodeMetrics.get(
                                  topologyMetricKey(
                                    nodeProps.payload.kind ?? "team",
                                    nodeProps.payload.name ?? ""
                                  )
                                )
                              : undefined
                          }
                          revision={topologyRevision}
                          onHoverChange={setTopologyTooltip}
                        />
                      )}
                      link={(linkProps: TopologyLinkRendererProps) => (
                        <TopologyLink
                          {...linkProps}
                          revision={topologyRevision}
                          onHoverChange={setTopologyTooltip}
                        />
                      )}
                      nodePadding={28}
                      nodeWidth={14}
                      linkCurvature={0.52}
                      margin={{ top: 10, right: 34, bottom: 10, left: 34 }}
                      sort={false}
                    />
                  </ResponsiveContainer>
                  </div>
                </div>
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
