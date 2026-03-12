"use client";

import { Copy, Download, RefreshCw } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import type { UsageRecord } from "@/components/dashboard/types";

type RecordsTabProps = {
  records: UsageRecord[];
  total: number;
  limit: number;
  offset: number;
  loading: boolean;
  refreshing: boolean;
  pendingNewRecords: number;
  copiedRequestId: string | null;
  onRefresh: () => void;
  onJumpToLatest: () => void;
  onPageChange: (offset: number) => void;
  onExport: () => void;
  onCopyRequestId: (requestId: string) => void;
  onSelectRecord: (record: UsageRecord) => void;
};

const HIGH_LATENCY_THRESHOLD_MS = 3000;

export function RecordsTab({
  records,
  total,
  limit,
  offset,
  loading,
  refreshing,
  pendingNewRecords,
  copiedRequestId,
  onRefresh,
  onJumpToLatest,
  onPageChange,
  onExport,
  onCopyRequestId,
  onSelectRecord,
}: RecordsTabProps) {
  const currentPage = Math.floor(offset / limit) + 1;
  const totalPages = Math.max(1, Math.ceil(total / limit));

  return (
    <Card className="rounded-[22px] border-[#111827] bg-white">
      <CardHeader className="gap-4 lg:flex-row lg:items-end lg:justify-between">
        <div>
          <CardTitle>Raw Usage Records</CardTitle>
          <CardDescription>
            Request-level traces with routing provenance, latency, token usage, and provider diagnostics.
          </CardDescription>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="outline" onClick={onExport}>
            <Download className="size-4" />
            Export CSV
          </Button>
          <Button onClick={onRefresh}>
            <RefreshCw className={`size-4 ${refreshing ? "animate-spin" : ""}`} />
            Refresh
          </Button>
        </div>
      </CardHeader>
      <CardContent className="space-y-4">
        {pendingNewRecords > 0 ? (
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-[#d8dee7] bg-[#f8fafc] px-4 py-3 text-sm text-[#334155]">
            <div>{pendingNewRecords} new records available. Jump back to page 1 to inspect them.</div>
            <Button
              variant="outline"
              className="border-[#cdd5df] bg-white text-[#334155] hover:bg-[#f8fafc]"
              onClick={onJumpToLatest}
            >
              View Latest
            </Button>
          </div>
        ) : null}

        <div className="overflow-hidden rounded-2xl border border-[#d8dee7]">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Time</TableHead>
                <TableHead>Request ID</TableHead>
                <TableHead>Team</TableHead>
                <TableHead>Route Info</TableHead>
                <TableHead>Model</TableHead>
                <TableHead>Tokens In/Out</TableHead>
                <TableHead>Latency</TableHead>
                <TableHead>Status</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {loading ? (
                <TableRow>
                  <TableCell colSpan={8} className="py-10 text-center text-sm text-[#64748b]">
                    Loading records...
                  </TableCell>
                </TableRow>
              ) : records.length === 0 ? (
                <TableRow>
                  <TableCell colSpan={8} className="py-10 text-center text-sm text-[#64748b]">
                    No usage records matched the current filters.
                  </TableCell>
                </TableRow>
              ) : (
                records.map((record) => {
                  const latency = record.latency_ms ?? 0;
                  const requestId = record.request_id ?? `record-${record.id}`;
                  const statusTone = record.status.includes("error")
                    ? "bg-[#fee2e2] text-[#b91c1c]"
                    : "bg-[#dcfce7] text-[#047857]";

                  return (
                    <TableRow
                      key={record.id}
                      className="cursor-pointer hover:bg-[#f8fafc]"
                      onClick={() => onSelectRecord(record)}
                    >
                      <TableCell className="font-mono text-xs text-[#64748b]">{record.timestamp}</TableCell>
                      <TableCell>
                        <div className="flex items-center gap-2">
                          <button
                            type="button"
                            className="font-mono text-xs text-[#17233c] hover:text-[#334155]"
                            onClick={(event) => {
                              event.stopPropagation();
                              onCopyRequestId(requestId);
                            }}
                          >
                            {requestId}
                          </button>
                          <Copy className="size-3.5 text-slate-400" />
                          {copiedRequestId === requestId ? (
                            <span className="text-[11px] font-medium text-[#047857]">Copied</span>
                          ) : null}
                        </div>
                      </TableCell>
                      <TableCell>{record.team_id}</TableCell>
                      <TableCell>
                        <div className="space-y-1">
                          <div className="text-sm font-medium text-[#17233c]">{record.router}</div>
                          <div className="text-xs text-[#64748b]">
                            rule: {record.matched_rule ?? "fallback"}
                          </div>
                          <div className="text-xs text-[#64748b]">
                            final: {record.final_channel}
                          </div>
                        </div>
                      </TableCell>
                      <TableCell>{record.model}</TableCell>
                      <TableCell className="text-xs text-[#64748b]">
                        {record.input_tokens.toLocaleString()} / {record.output_tokens.toLocaleString()}
                      </TableCell>
                      <TableCell>
                        <span
                          className={`rounded-full px-2 py-1 text-xs font-medium ${
                            latency >= HIGH_LATENCY_THRESHOLD_MS
                              ? "bg-[#fee2e2] text-[#b91c1c]"
                              : "bg-[#e2e8f0] text-[#475569]"
                          }`}
                        >
                          {record.latency_ms ? `${record.latency_ms.toFixed(1)} ms` : "n/a"}
                        </span>
                      </TableCell>
                      <TableCell>
                        <span className={`rounded-full px-2 py-1 text-xs font-medium ${statusTone}`}>
                          {record.status}
                        </span>
                      </TableCell>
                    </TableRow>
                  );
                })
              )}
            </TableBody>
          </Table>
        </div>

        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="text-sm text-[#64748b]">
            Page {currentPage} of {totalPages} · {total.toLocaleString()} total records
          </div>
          <div className="flex items-center gap-2">
            <Button variant="outline" disabled={offset === 0 || loading} onClick={() => onPageChange(Math.max(0, offset - limit))}>
              Previous
            </Button>
            <Button
              variant="outline"
              disabled={offset + limit >= total || loading}
              onClick={() => onPageChange(offset + limit)}
            >
              Next
            </Button>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
