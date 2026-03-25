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
import { Button } from "@/components/ui/button";
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

type TopologyPresentation = {
  mode: "sankey" | "summary";
  note?: string;
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

type TopologyFlowSummaryStats = {
  totalRequests: number;
  totalTokens: number;
  flowCount: number;
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

function buildTopologyPresentation(
  topology: DashboardAnalyticsResponse["topology"]
): TopologyPresentation {
  if (topology.render_mode === "summary") {
    return {
      mode: "summary",
      note: "Compact view enabled because this routing graph collapses many nodes into a few middle stages.",
    };
  }

  const counts = {
    team: 0,
    router: 0,
    channel: 0,
    model: 0,
  };

  for (const node of topology.nodes) {
    if (node.kind in counts) {
      counts[node.kind as keyof typeof counts] += 1;
    }
  }

  const middleCount = Math.max(Math.max(counts.router, counts.channel), 1);
  const edgeCount = Math.max(Math.max(counts.team, counts.model), 1);
  const imbalanceRatio = edgeCount / middleCount;
  const denseTopology = topology.flows.length >= 8 || topology.links.length >= 12;
  const severeBottleneck =
    edgeCount >= 8 && middleCount <= 3 && imbalanceRatio >= 2.5 && denseTopology;

  if (severeBottleneck) {
    return {
      mode: "summary",
      note: "Compact view enabled because this routing graph collapses many nodes into a few middle stages.",
    };
  }

  return { mode: "sankey" };
}

function getTopologyNodePadding(topology: DashboardAnalyticsResponse["topology"]) {
  const counts = new Map<string, number>();

  for (const node of topology.nodes) {
    counts.set(node.kind, (counts.get(node.kind) ?? 0) + 1);
  }

  const maxColumnCount = Math.max(...counts.values(), 1);
  return Math.max(12, Math.min(28, Math.floor(220 / maxColumnCount)));
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
  totalRequests?: number;
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
    totalRequests = 0,
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
  const canShowPathLabel = labelSource.length > 0 && linkWidth >= 16 && span >= 220 && approxChars >= 14;
  const canShowSummary = !canShowPathLabel && linkWidth >= 13 && span >= 132;
  const label = canShowPathLabel ? fullPathLabel : canShowSummary ? summaryLabel : "";
  const labelOpacity = truncated ? 0.92 : 1;
  const share = totalRequests > 0 ? (payload?.value ?? 0) / totalRequests : 0;
  const baseStrokeOpacity = Math.max(0.28, Math.min(0.96, 0.28 + share * 2.6));
  const innerStrokeOpacity = Math.max(0.18, Math.min(0.68, 0.18 + share * 1.8));
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
        strokeOpacity={baseStrokeOpacity}
        strokeWidth={linkWidth}
        strokeLinecap="round"
      />
      <path
        d={path}
        fill="none"
        stroke="#efc89a"
        strokeOpacity={innerStrokeOpacity}
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

function buildTopologyFlowSummaryStats(
  flows: DashboardAnalyticsResponse["topology"]["flows"]
): TopologyFlowSummaryStats {
  return flows.reduce(
    (accumulator, flow) => {
      accumulator.totalRequests += flow.requests;
      accumulator.totalTokens += flow.total_tokens;
      accumulator.flowCount += 1;
      return accumulator;
    },
    { totalRequests: 0, totalTokens: 0, flowCount: 0 }
  );
}

function TopologyFlowSummary({
  flows,
}: {
  flows: DashboardAnalyticsResponse["topology"]["flows"];
}) {
  const sortedFlows = [...flows].sort((left, right) => {
    if (right.requests !== left.requests) {
      return right.requests - left.requests;
    }

    return right.total_tokens - left.total_tokens;
  });
  const visibleFlows = sortedFlows.slice(0, 8);
  const remainingFlows = sortedFlows.length - visibleFlows.length;
  const stats = buildTopologyFlowSummaryStats(flows);

  return (
    <div className="space-y-3" data-topology-summary>
      <div className="grid gap-3 md:grid-cols-3">
        <div className="rounded-2xl border border-[#f1dfc3] bg-[#fff8ee] px-4 py-3">
          <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-[#94a3b8]">
            Flows
          </div>
          <div className="mt-2 text-2xl font-semibold text-[#8e3f1d]">{formatInteger(stats.flowCount)}</div>
        </div>
        <div className="rounded-2xl border border-[#f1dfc3] bg-[#fff8ee] px-4 py-3">
          <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-[#94a3b8]">
            Requests
          </div>
          <div className="mt-2 text-2xl font-semibold text-[#8e3f1d]">{formatInteger(stats.totalRequests)}</div>
        </div>
        <div className="rounded-2xl border border-[#f1dfc3] bg-[#fff8ee] px-4 py-3">
          <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-[#94a3b8]">
            Tokens
          </div>
          <div className="mt-2 text-2xl font-semibold text-[#8e3f1d]">{formatCompact(stats.totalTokens)}</div>
        </div>
      </div>
      <div className="flex items-center justify-between px-1 text-xs text-[#64748b]">
        <span>Showing the busiest routing paths first.</span>
        {remainingFlows > 0 ? <span>{`${remainingFlows} more flows hidden`}</span> : null}
      </div>
      <div className="grid gap-3 md:grid-cols-2">
        {visibleFlows.map((flow) => (
          <div
            key={`${flow.team_id}:${flow.router}:${flow.channel}:${flow.model}`}
            className="rounded-[20px] border border-[#e2e8f0] bg-[#fcf7ef] p-4 shadow-[0_6px_18px_rgba(148,85,26,0.06)]"
          >
            <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-[#94a3b8]">
              Flow
            </div>
            <div className="mt-3 flex flex-wrap items-center gap-2 text-xs">
              <span className="rounded-full bg-[#fff1db] px-3 py-1 font-medium text-[#8e3f1d]">
                {flow.team_id}
              </span>
              <span className="text-[#c2410c]">→</span>
              <span className="rounded-full bg-white px-3 py-1 font-medium text-[#475569]">
                {flow.router}
              </span>
              <span className="text-[#c2410c]">→</span>
              <span className="rounded-full bg-white px-3 py-1 font-medium text-[#475569]">
                {flow.channel}
              </span>
            </div>
            <div className="mt-3 rounded-xl bg-white/80 px-3 py-2 text-sm text-[#475569]">
              Model <span className="text-[#c2410c]">·</span> {flow.model}
            </div>
            <div className="mt-4 flex items-center justify-between border-t border-[#eadfce] pt-3 text-xs text-[#64748b]">
              <span>{formatInteger(flow.requests)} requests</span>
              <span>{formatInteger(flow.total_tokens)} tokens</span>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

export function OverviewTab({ analytics }: OverviewTabProps) {
  const [topologyTooltip, setTopologyTooltip] = useState<TopologyHoverOverlay | null>(null);
  const [topologyViewState, setTopologyViewState] = useState<{
    revision: string;
    mode: "sankey" | "summary";
  } | null>(null);
  const topologyRevision = analytics
    ? `${analytics.generated_at}:${analytics.topology.nodes.length}:${analytics.topology.links.length}`
    : "empty";

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
  const topologyPresentation = buildTopologyPresentation(analytics.topology);
  const topologyNodePadding = getTopologyNodePadding(analytics.topology);
  const effectiveTopologyMode =
    topologyViewState?.revision === topologyRevision
      ? topologyViewState.mode
      : topologyPresentation.mode;

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
                <div className="mb-4 flex items-center justify-between gap-3">
                  <div className="text-xs uppercase tracking-[0.16em] text-[#94a3b8]">
                    View Mode
                  </div>
                  <div className="flex items-center gap-2" data-topology-view-toggle>
                    <Button
                      size="sm"
                      variant={effectiveTopologyMode === "sankey" ? "secondary" : "outline"}
                      onClick={() =>
                        setTopologyViewState({ revision: topologyRevision, mode: "sankey" })
                      }
                      aria-pressed={effectiveTopologyMode === "sankey"}
                    >
                      Sankey
                    </Button>
                    <Button
                      size="sm"
                      variant={effectiveTopologyMode === "summary" ? "secondary" : "outline"}
                      onClick={() =>
                        setTopologyViewState({ revision: topologyRevision, mode: "summary" })
                      }
                      aria-pressed={effectiveTopologyMode === "summary"}
                    >
                      Compact
                    </Button>
                  </div>
                </div>
                {effectiveTopologyMode === "sankey" ? (
                  <>
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
                                totalRequests={analytics.overview.total_requests}
                              />
                            )}
                            nodePadding={topologyNodePadding}
                            nodeWidth={12}
                            linkCurvature={0.38}
                            iterations={64}
                            margin={{ top: 10, right: 34, bottom: 10, left: 34 }}
                            sort
                          />
                        </ResponsiveContainer>
                      </div>
                    </div>
                  </>
                ) : (
                  <div className="space-y-4">
                    {topologyPresentation.mode === "summary" && topologyPresentation.note ? (
                      <div className="rounded-2xl border border-[#f3e2c7] bg-[#fff8ee] px-4 py-3 text-sm text-[#8e3f1d]">
                        {topologyPresentation.note}
                      </div>
                    ) : null}
                    <TopologyFlowSummary flows={analytics.topology.flows} />
                  </div>
                )}
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
